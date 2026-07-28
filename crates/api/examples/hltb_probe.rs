fn main() {
    let client = vapourfly_api::hltb::HltbClient::new();
    for name in ["Celeste", "The Witcher 3: Wild Hunt", "Counter-Strike 2"] {
        match client.fetch(name) {
            Ok(Some(d)) => println!(
                "{name}: main={:?}s extra={:?}s 100%={:?}s",
                d.main_story_seconds, d.main_extra_seconds, d.completionist_seconds
            ),
            Ok(None) => println!("{name}: no match"),
            Err(e) => println!("{name}: ERR {e}"),
        }
    }
}
