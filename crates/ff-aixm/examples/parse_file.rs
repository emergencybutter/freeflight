//! Smoke-test / inspect the AIXM 4.5 parser against a real file:
//!
//! ```sh
//! cargo run -p ff-aixm --example parse_file -- <path/to/AIXM4.5_all_FR_OM_*.xml> [region]
//! ```
//!
//! Prints feature counts and a sample of each kind — a quick way to sanity
//! -check a new state's export before wiring it into `ff-etl`.
fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: parse_file <aixm.xml> [region=LF]");
    let region = args.next().unwrap_or_else(|| "LF".to_string());

    let bytes = std::fs::read(&path).expect("read AIXM file");
    let data = ff_aixm::parse_snapshot(&bytes, &region).expect("parse AIXM snapshot");

    println!("airports:  {}", data.airports.len());
    println!("navaids:   {}", data.navaids.len());
    println!("waypoints: {}", data.waypoints.len());
    println!("runways:   {}", data.runways.len());
    println!("airways:   {} ({} legs)", data.airways.len(), data.airway_legs.len());
    println!("airspaces: {}", data.airspaces.len());

    if let Some(a) = data
        .airports
        .iter()
        .find(|a| a.icao == "LFPG")
        .or_else(|| data.airports.first())
    {
        println!(
            "  e.g. airport  {} \"{}\" ({:.5}, {:.5}) {} ft type={:?}",
            a.icao, a.name, a.lat, a.lon, a.elevation_ft, a.airport_type
        );
    }
    if let Some(n) = data.navaids.first() {
        println!(
            "  e.g. navaid   {} {:?} {:?} kHz ({:.5}, {:.5})",
            n.ident, n.navaid_type, n.freq_khz, n.lat, n.lon
        );
    }
    if let Some(w) = data.waypoints.first() {
        println!("  e.g. waypoint {} ({:.5}, {:.5})", w.ident, w.lat, w.lon);
    }
    if let Some(r) = data
        .runways
        .iter()
        .find(|r| r.airport_icao == "LFRC")
        .or_else(|| data.runways.first())
    {
        println!(
            "  e.g. runway   {} {} {}x{} ft {:?}  {}@{:.0}° / {}@{:.0}°",
            r.airport_icao,
            r.ident,
            r.length_ft,
            r.width_ft,
            r.surface,
            r.low_end.ident,
            r.low_end.heading_deg,
            r.high_end.ident,
            r.high_end.heading_deg
        );
    }
    if let Some(a) = data
        .airways
        .iter()
        .find(|a| a.ident == "L615")
        .or_else(|| data.airways.first())
    {
        let fixes: Vec<&str> = data
            .airway_legs
            .iter()
            .filter(|l| l.airway_ident == a.ident)
            .map(|l| l.fix_ident.as_str())
            .collect();
        println!("  e.g. airway   {} {:?}  {}", a.ident, a.kind, fixes.join(" → "));
    }
}
