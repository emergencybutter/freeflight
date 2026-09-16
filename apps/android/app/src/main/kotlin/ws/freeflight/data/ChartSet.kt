package ws.freeflight.data

import uniffi.ff_uniffi.BoundingBox
import uniffi.ff_uniffi.Chart

/**
 * A named group of charts a pilot can take in one action.
 *
 * Downloading charts one at a time is the honest mechanism — each is tens
 * to hundreds of megabytes and the offline client makes every transfer an
 * explicit choice (DESIGN.md §8) — but it is a poor interface. A
 * nationwide cycle publishes 111 archives, and nobody wants to tap 36
 * checkboxes to cover the lower 48. A set keeps the choice explicit while
 * making it one decision instead of dozens.
 *
 * The groupings here are derived from the catalogue rather than
 * hand-curated: chart *kind*, which the bundle already records, and the
 * area the map is showing, which is the question a pilot actually asks
 * ("what covers where I'm flying"). No invented region taxonomy — a
 * "Pacific Northwest" list would be someone's opinion baked into the app,
 * and wrong at its edges.
 */
data class ChartSet(
    val name: String,
    val description: String,
    val charts: List<Chart>,
) {
    val installedCount: Int get() = charts.count { it.installed }

    /** Charts still to fetch — the set minus whatever is already here. */
    val missing: List<Chart> get() = charts.filterNot { it.installed }

    /**
     * Bytes this set would still cost, or null if any missing chart has no
     * published size (a bundle older than migration 0008). Null means "say
     * unknown", never "assume zero".
     */
    val remainingBytes: Long?
        get() {
            val sizes = missing.map { it.downloadBytes }
            if (sizes.any { it == null }) return null
            return sizes.sumOf { it!!.toLong() }
        }

    val isComplete: Boolean get() = charts.isNotEmpty() && missing.isEmpty()

    companion object {
        /**
         * The sets offered for a catalogue, most useful first.
         *
         * `viewport` is the map's current bounds when there is one; it
         * drives the "covering the map view" set, which is the only one
         * that needs geography. Sets that would be empty are dropped
         * rather than shown as a zero — an empty "IFR High" entry on a
         * cycle without those charts is noise.
         */
        fun forCatalogue(charts: List<Chart>, viewport: BoundingBox?): List<ChartSet> {
            if (charts.isEmpty()) return emptyList()

            val sets = mutableListOf<ChartSet>()

            viewport?.let { bounds ->
                val covering = charts.filter { it.bbox.intersects(bounds) }
                if (covering.isNotEmpty()) {
                    sets += ChartSet(
                        name = "Covering the map view",
                        description = "Every chart that overlaps what the map is showing",
                        charts = covering,
                    )
                }
            }

            // Grouped by the `kind` the bundle records, so this tracks
            // whatever ff-etl publishes rather than a list kept in step by
            // hand.
            charts.groupBy { it.kind }
                .toList()
                .sortedBy { (kind, _) -> ChartKinds.order(kind) }
                .forEach { (kind, group) ->
                    sets += ChartSet(
                        name = "All ${ChartKinds.label(kind)} charts",
                        description = "${group.size} charts",
                        charts = group,
                    )
                }

            sets += ChartSet(
                name = "Everything",
                description = "Every chart this cycle publishes",
                charts = charts,
            )
            return sets
        }

    }
}

/** Overlap, not containment — a chart far larger than the viewport still covers it. */
private fun BoundingBox.intersects(other: BoundingBox): Boolean =
    maxLat >= other.minLat &&
        minLat <= other.maxLat &&
        maxLon >= other.minLon &&
        minLon <= other.maxLon
