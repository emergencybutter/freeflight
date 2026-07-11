//! Validates neatline detection against real-chart previews staged in
//! FF_NEATLINE_PREVIEW_DIR (rendered exactly as crop_to_neatline does:
//! `gdal_translate -outsize 4000 0 -r average`). Ignored by default —
//! it needs those staged files; see the session notes / crop_to_neatline
//! docs for how they were produced. Covers all four chart families the
//! neatline crop applies to: TAC, VFR Flyway, IFR Enroute, and
//! Helicopter (east/west) — the last is why detection went rows-first:
//! a real LA Heli chart's legend abuts the map body with no vertical
//! border of its own.
#[test]
#[ignore]
fn probe_real_previews() {
    let dir = std::env::var("FF_NEATLINE_PREVIEW_DIR").unwrap();
    for name in [
        "nl4_tac.png",
        "nl4_fly.png",
        "nl4_enr.png",
        "nl4_heli_east.png",
        "nl4_heli_west.png",
    ] {
        let img = image::open(format!("{dir}/{name}")).unwrap().into_rgb8();
        let (w, h) = img.dimensions();
        let boxed = ff_etl::chart_prep::detect_neatline_for_probe(&img);
        println!(
            "{name}: {w}x{h} -> {boxed:?} fractions {:?}",
            boxed.map(|(l, t, r, b)| (
                (l as f64 / w as f64 * 1000.0).round() / 1000.0,
                (t as f64 / h as f64 * 1000.0).round() / 1000.0,
                (r as f64 / w as f64 * 1000.0).round() / 1000.0,
                (b as f64 / h as f64 * 1000.0).round() / 1000.0
            ))
        );
        assert!(boxed.is_some(), "{name}: no neatline detected");
    }
}
