package ws.freeflight.data

/**
 * Labels and ordering for `chart_catalog.kind`, matching what the web
 * client shows (`CHART_KIND_LABELS` / `CHART_KIND_ORDER` in
 * `apps/web/src/MapView.tsx`).
 *
 * Both clients read the same catalogue and both let a pilot pick a *chart
 * series* rather than an individual sheet, so the vocabulary should be the
 * same on each — someone who knows "IFR Low" on the web should not have to
 * learn "IFR Enroute Low charts" on the phone. Kept deliberately terse:
 * these appear in a map control sitting on top of a chart.
 *
 * The raw kind strings come from `chart_kind_str` in
 * `services/ff-etl/src/bundle.rs`. Anything not listed falls back to the
 * raw string and sorts last, so a new kind appearing in a future cycle
 * shows up unlabelled rather than disappearing.
 */
object ChartKinds {

    private val LABELS = mapOf(
        "Sectional" to "Sectional",
        "TerminalAreaChart" to "TAC",
        "VfrFlyway" to "Flyway",
        "HelicopterRoute" to "Heli",
        "IfrEnrouteLow" to "IFR Low",
        "IfrEnrouteHigh" to "IFR High",
    )

    // VFR broad → terminal, then IFR, then the specialty heli charts.
    private val ORDER = mapOf(
        "Sectional" to 0,
        "TerminalAreaChart" to 1,
        "VfrFlyway" to 2,
        "IfrEnrouteLow" to 3,
        "IfrEnrouteHigh" to 4,
        "HelicopterRoute" to 5,
    )

    /** The kind a fresh install draws, when it has one. */
    const val DEFAULT = "Sectional"

    fun label(kind: String): String = LABELS[kind] ?: kind

    fun order(kind: String): Int = ORDER[kind] ?: 99

    /** Kinds present in `charts`, in the order the selector should list them. */
    fun ordered(kinds: Collection<String>): List<String> =
        kinds.distinct().sortedWith(compareBy({ order(it) }, { it }))
}
