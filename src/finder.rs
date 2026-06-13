//! offset finder for re-deriving memory locations after a shift

#[cfg(target_os = "windows")]
use crate::memory::{
    is_plausible_ptr, load_offsets, Offsets, ProcessMemory, TourneyClientProcess, TourneyReader,
};

type FinderError = Box<dyn std::error::Error + Send + Sync>;

#[cfg(target_os = "windows")]
pub fn run_finder() -> Result<(), FinderError> {
    use std::io::{BufRead, Write};

    let offsets = load_offsets().map_err(|e| -> FinderError { e.into() })?;

    let pids = TourneyReader::find_osu_processes()?;
    if pids.is_empty() {
        return Err("no osu! processes found".into());
    }
    let titles = TourneyReader::enumerate_osu_windows();

    let mut clients: Vec<(i32, u32)> = match TourneyReader::identify_tourney_processes(&pids) {
        Ok((_, c)) if !c.is_empty() => c,
        _ => pids.iter().map(|&p| (0, p)).collect(),
    };
    clients.retain(|(_, pid)| {
        !titles
            .get(pid)
            .is_some_and(|t| TourneyReader::is_lazer_title(t))
    });
    if clients.is_empty() {
        return Err("no attachable osu! clients (all filtered as lazer)".into());
    }

    println!("detected clients:");
    for (i, (slot, pid)) in clients.iter().enumerate() {
        let title = titles.get(pid).map(|s| s.trim()).unwrap_or("");
        println!("  [{}] slot {} (pid {}) \"{}\"", i, slot, pid, title);
    }

    let stdin = std::io::stdin();
    let read_line = |prompt: &str| -> std::io::Result<Option<String>> {
        print!("{}", prompt);
        std::io::stdout().flush()?;
        let mut line = String::new();
        if stdin.lock().read_line(&mut line)? == 0 {
            return Ok(None);
        }
        Ok(Some(line.trim().to_string()))
    };

    let idx: usize = match read_line("select client index: ")? {
        Some(s) => s.parse().map_err(|_| "invalid index")?,
        None => return Ok(()),
    };
    let (slot, pid) = *clients.get(idx).ok_or("index out of range")?;
    let client = TourneyReader::init_client_process(pid, slot, &offsets)?;

    println!("\nattached to slot {} (pid {}).", slot, pid);
    println!("options:");
    println!("  <number>         discard matches not equal to the provided number");
    println!("  r [depth] [off]  resolve candidates to offset paths (default depth 6, off 0x400)");
    println!("  b                re-baseline");
    println!("  q                quit");

    let mut cands = client.process.scan_range(1, 2_000_000_000);
    println!("\nfound {} candidates, type in next value", cands.len());

    let parse_num = |s: &str| -> Option<usize> {
        match s.strip_prefix("0x") {
            Some(hex) => usize::from_str_radix(hex, 16).ok(),
            None => s.parse().ok(),
        }
    };

    loop {
        let cmd = match read_line("> ")? {
            Some(c) => c,
            None => break,
        };

        if cmd == "q" {
            break;
        } else if cmd.is_empty() {
            // nothing
        } else if cmd == "b" {
            cands = client.process.scan_range(1, 2_000_000_000);
            println!("found {} candidates, type in next value", cands.len());
        } else if cmd == "r" || cmd.starts_with("r ") {
            let mut args = cmd.split_whitespace().skip(1);
            let depth = args
                .next()
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(6);
            let off = args.next().and_then(parse_num).unwrap_or(0x400);
            finder_resolve(&client, &offsets, &cands, depth, off);
        } else if let Ok(target) = cmd.parse::<i32>() {
            let p = &client.process;
            cands.retain_mut(|(a, v)| match p.read_i32(*a) {
                Ok(cur) if cur == target => {
                    *v = cur;
                    true
                }
                _ => false,
            });
            println!("found {} candidates, type in next value", cands.len());
        } else {
            println!("unknown command: {:?}", cmd);
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn finder_resolve(
    client: &TourneyClientProcess,
    offsets: &Offsets,
    cands: &[(usize, i32)],
    max_depth: usize,
    max_off: usize,
) {
  if cands.len() > 10 {
        println!(
            "{} candidates - try to narrow further",
            cands.len()
        );
        return;
    }
    if cands.is_empty() {
        println!("no candidates");
        return;
    }

    let p = &client.process;
    let ruleset = match TourneyReader::resolve_ruleset(p, client.rulesets_addr, &offsets.ruleset) {
        Ok(r) => r,
        Err(_) => {
            println!("cannot resolve ruleset (is gameplay active?)");
            return;
        }
    };
    let gbase = p
        .read_ptr32(ruleset + offsets.ruleset.gameplay_ptr)
        .unwrap_or(0);
    let sbase = if gbase != 0 {
        p.read_ptr32(gbase + offsets.ruleset.gameplay_base)
            .unwrap_or(0)
    } else {
        0
    };
    let anchors: Vec<(&str, usize)> = [
        ("scoreBase", sbase),
        ("gameplayBase", gbase),
        ("ruleset", ruleset),
        ("rulesetsAddr", client.rulesets_addr),
        ("baseAddr", client.base_addr),
    ]
    .into_iter()
    .filter(|(_, a)| *a != 0)
    .collect();

    println!("anchors:");
    for (name, a) in &anchors {
        println!("  {:<12} 0x{:X}", name, a);
    }

    let mut pmap: Option<Vec<(usize, usize)>> = None;

    for (addr, val) in cands {
        println!("0x{:X} = {}", addr, val);

        let mut paths = finder_paths(p, &anchors, *addr);
        if paths.is_empty() {
            let map = pmap.get_or_insert_with(|| {
                println!("  building pointer map...");
                let mut m = p.scan_pointers();
                m.sort_unstable();
                m
            });
            paths = finder_pointer_paths(map, &anchors, *addr, max_depth, max_off);
        }

        if paths.is_empty() {
            let nearest = anchors
                .iter()
                .map(|(n, a)| (n, (*addr as isize - *a as isize)))
                .min_by_key(|(_, d)| d.abs());
            match nearest {
                Some((n, d)) => println!("    no path found (nearest anchor {} {:+} bytes)", n, d),
                None => println!("    no path found"),
            }
        } else {
            for (name, offs) in paths.iter().take(12) {
                let arr = offs
                    .iter()
                    .map(|o| o.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                let tag = if *name == "ruleset" && offs.len() == 1 {
                    format!("   <- gameplay_score_spectator = {}", offs[0])
                } else {
                    String::new()
                };
                println!("    {} + [{}]{}", name, arr, tag);
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn finder_pointer_paths<'a>(
    map: &[(usize, usize)],
    anchors: &[(&'a str, usize)],
    target: usize,
    max_depth: usize,
    max_off: usize,
) -> Vec<(&'a str, Vec<usize>)> {
    use std::collections::HashSet;

    let mut results = Vec::new();
    let mut visited: HashSet<usize> = HashSet::new();
    let mut frontier: Vec<(usize, Vec<usize>)> = vec![(target, Vec::new())];

    for _ in 0..=max_depth {
        let mut next: Vec<(usize, Vec<usize>)> = Vec::new();
        for (node, suffix) in &frontier {
            for (name, anchor) in anchors {
                if *node >= *anchor && *node - *anchor <= max_off {
                    let mut offs = vec![*node - *anchor];
                    offs.extend(suffix.iter().copied());
                    results.push((*name, offs));
                }
            }
            if results.len() >= 12 {
                break;
            }
            let lo = node.saturating_sub(max_off);
            let start = map.partition_point(|(v, _)| *v < lo);
            for &(value, holder) in &map[start..] {
                if value > *node {
                    break;
                }
                if visited.insert(holder) {
                    let mut offs = vec![*node - value];
                    offs.extend(suffix.iter().copied());
                    next.push((holder, offs));
                }
            }
        }
        if !results.is_empty() {
            break;
        }
        if next.len() > 20000 {
            next.truncate(20000);
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }
    results
}

#[cfg(target_os = "windows")]
fn finder_paths<'a>(
    p: &ProcessMemory,
    anchors: &[(&'a str, usize)],
    target: usize,
) -> Vec<(&'a str, Vec<usize>)> {
    const W: usize = 0x400;
    const W2: usize = 0x100;
    let mut out = Vec::new();

    for (name, anchor) in anchors {
        if target >= *anchor && target - *anchor < W {
            out.push((*name, vec![target - anchor]));
        }
    }
    for (name, anchor) in anchors {
        for off in (0..W).step_by(4) {
            if let Ok(p1) = p.read_ptr32(anchor + off) {
                if is_plausible_ptr(p1) && target >= p1 && target - p1 < W {
                    out.push((*name, vec![off, target - p1]));
                }
            }
        }
    }
    for (name, anchor) in anchors {
        for off1 in (0..W2).step_by(4) {
            let Ok(p1) = p.read_ptr32(anchor + off1) else {
                continue;
            };
            if !is_plausible_ptr(p1) {
                continue;
            }
            for off2 in (0..W2).step_by(4) {
                if let Ok(p2) = p.read_ptr32(p1 + off2) {
                    if is_plausible_ptr(p2) && target >= p2 && target - p2 < W2 {
                        out.push((*name, vec![off1, off2, target - p2]));
                    }
                }
            }
        }
    }
    out
}

#[cfg(not(target_os = "windows"))]
pub fn run_finder() -> Result<(), FinderError> {
    Err("find-offset is only supported on Windows".into())
}
