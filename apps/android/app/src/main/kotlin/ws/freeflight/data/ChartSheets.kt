package ws.freeflight.data

import uniffi.ff_uniffi.Chart

/**
 * What one chart sheet is called in the UI, and the order sheets appear in.
 *
 * Every chart kind but one is already named after a place — "Albuquerque
 * Sectional", "Boston TAC", "Chicago Helicopter" — and those names are
 * used as they come. The IFR enroute series is the exception: the
 * catalogue records the FAA's own file stem, so a pilot choosing what to
 * download reads a list of 51 entries called "IFR Low Altitude Enroute
 * enr_l01" through "enr_l36", which identifies nothing.
 *
 * Worse, three of those names were *duplicated* — a panel can ship
 * several parts (L-6 is published as north and south halves; L-23 and
 * L-34 each carry an inset) and every part was named after the panel, so
 * two distinct downloads appeared under one identical label. That is
 * fixed in `ff-etl` for future cycles, but cycles already on a device
 * carry the old names, and the id never had the problem — so the label
 * here is derived from the id, which is precise in both.
 *
 * The rendering uses the FAA's own designator ("L-6 North", "H-12")
 * rather than inventing region names. Which sheet covers a given route is
 * a question the "Covering the map view" set answers properly; a name
 * cannot.
 */
object ChartSheets {

    // Slug shapes seen across a full cycle: enr_l01, enr_l06n, enr_l06s,
    // enr_l23_wilm_inset, enr_l34_bost_inset, enr_h12 — all 51 IFR charts
    // in the 2026-08-06 catalogue match this.
    private val IFR = Regex("""^enr_([lh])(\d+)([ns])?(?:_(.+)_inset)?$""")

    private val ID_PREFIX = Regex("""^\d{4}-\d{2}-\d{2}-""")

    /** FAA abbreviations for the cities whose insets the series carries. */
    private val INSET_PLACES = mapOf("wilm" to "Wilmington", "bost" to "Boston")

    /**
     * The sheet's name for a list a pilot chooses from.
     *
     * Falls back to the catalogue name whenever the id isn't an IFR slug,
     * which is every other kind and any future series this doesn't know.
     */
    fun label(chart: Chart): String = ifrLabel(slugOf(chart.id)) ?: chart.name

    /**
     * Sort key within a kind: IFR sheets by panel number (so L-2 precedes
     * L-10, which sorting the text would not), everything else by name.
     *
     * Zero-padded rather than a numeric field because a kind is either all
     * IFR or none of it, so one string key orders both cases correctly.
     */
    fun sortKey(chart: Chart): String {
        val match = IFR.matchEntire(slugOf(chart.id))
            ?: return chart.name.lowercase()
        // Halves and insets sort directly after the panel they belong to.
        val suffix = match.groupValues[3] + match.groupValues[4]
        return "%03d%s".format(match.groupValues[2].toInt(), suffix)
    }

    private fun slugOf(chartId: String): String = ID_PREFIX.replace(chartId, "")

    private fun ifrLabel(slug: String): String? {
        val match = IFR.matchEntire(slug) ?: return null
        val series = match.groupValues[1].uppercase()
        val number = match.groupValues[2].toInt()
        val half = when (match.groupValues[3]) {
            "n" -> " North"
            "s" -> " South"
            else -> ""
        }
        val inset = match.groupValues[4].takeIf { it.isNotEmpty() }?.let { token ->
            // An unrecognised abbreviation is shown as-is rather than
            // dropped: "L-23 inset (xyz)" still distinguishes it from
            // "L-23", which is the whole job.
            " inset (${INSET_PLACES[token] ?: token.replaceFirstChar(Char::uppercase)})"
        } ?: ""
        return "$series-$number$half$inset"
    }
}
