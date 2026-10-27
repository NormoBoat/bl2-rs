//! Compares merged Default*.ini chains with the Willow*.ini files the original game wrote (plan/12-tasks.md T-032b).
//! Run: $env:BL2_INSTALL="..."; $env:BL2_USER_CONFIG="<copy of My Games\Borderlands 2\WillowGame\Config>";
//!      cargo test -p bl2-formats --test ini_compare -- --ignored --nocapture

use std::collections::BTreeSet;
use std::path::PathBuf;

use bl2_formats::ini::Ini;

const FILES: [&str; 9] = ["Engine", "Game", "Input", "UI", "Lightmass", "Spark", "Steamworks", "Compat", "DLCManager"];

/// Written by the running game itself (timestamps, detected hardware, user settings, SaveConfig), not by merging.
fn written_at_runtime(section: &str, key: &str) -> bool {
    matches!(section, "iniversion" | "appcompat" | "systemsettings" | "defaultplayer" | "engine.playercontroller" | "willowgame.willowplayercontroller")
        || (section, key) == ("willowgame.willowgameengine", "bmuteaudiowhennotinfocus")
}

#[test]
#[ignore = "needs BL2_INSTALL and BL2_USER_CONFIG"]
fn merged_defaults_match_generated_files() {
    let install = PathBuf::from(std::env::var("BL2_INSTALL").expect("set BL2_INSTALL"));
    let user = PathBuf::from(std::env::var("BL2_USER_CONFIG").expect("set BL2_USER_CONFIG"));
    let (mut total_diffs, mut merge_diffs) = (0, 0);
    for f in FILES {
        let merged = Ini::load(&install.join(format!("WillowGame/Config/Default{f}.ini"))).unwrap();
        let generated = Ini::parse_generated(&std::fs::read_to_string(user.join(format!("Willow{f}.ini"))).unwrap());
        let sections: BTreeSet<String> = merged.sections.iter().chain(&generated.sections).map(|(n, _)| n.to_lowercase()).collect();
        let mut diffs = Vec::new();
        for s in &sections {
            let keys: BTreeSet<String> = [merged.section(s), generated.section(s)]
                .into_iter()
                .flatten()
                .flat_map(|v| v.iter().map(|(k, _)| k.to_lowercase()))
                .collect();
            for k in &keys {
                let (a, b) = (merged.values(s, k), generated.values(s, k));
                if a != b {
                    if !written_at_runtime(s, k) {
                        merge_diffs += 1;
                    }
                    let first = (0..a.len().max(b.len())).find(|&i| a.get(i) != b.get(i)).unwrap();
                    diffs.push(format!("[{s}] {k} ({} vs {} values, first difference at {first}: {:?} vs {:?}): merged={a:?} generated={b:?}", a.len(), b.len(), a.get(first), b.get(first)));
                }
            }
        }
        println!("Willow{f}.ini: sections={} differing_keys={}", sections.len(), diffs.len());
        for d in &diffs {
            println!("  {}", &d[..d.len().min(600)]);
        }
        total_diffs += diffs.len();
    }
    println!("TOTAL differing keys: {total_diffs}, of them not explained by runtime writes: {merge_diffs}");
    assert_eq!(merge_diffs, 0);
}
