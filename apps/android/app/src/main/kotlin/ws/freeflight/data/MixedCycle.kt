package ws.freeflight.data

import uniffi.ff_uniffi.DataSourceCredit

/** One source in the bundle that isn't from the bundle's own AIRAC cycle. */
data class StaleSource(val name: String, val effectiveDate: String)

/**
 * Which of a cycle's data sources are older than the cycle itself.
 *
 * A bundle is allowed to mix AIRAC cycles: the FAA feeds are fetched
 * automatically, while a national AIS export (France/SIA today) is a
 * manual download, so one side trailing the other by a cycle is routine.
 * `ff-etl` no longer refuses that — it records each source's own
 * effective date instead.
 *
 * Which makes this the client's side of the bargain. The cycle date shown
 * on the map and the Data screen is the *FAA* date; without this, that
 * date would read as a blanket claim covering French airports too, and a
 * pilot would have no way to know otherwise short of opening Settings
 * (§11: data freshness is explicit, never silent). A source with no date
 * at all can't be compared and is ignored here — `ff-etl`'s validation
 * refuses to publish one, so it shouldn't exist in a released bundle.
 */
object MixedCycle {

    fun staleSources(credits: List<DataSourceCredit>, cycleDate: String?): List<StaleSource> {
        if (cycleDate.isNullOrBlank()) return emptyList()
        return credits
            .mapNotNull { credit ->
                val effective = credit.effectiveDate?.takeIf { it.isNotBlank() } ?: return@mapNotNull null
                // Only *older* counts. A source ahead of the FAA cycle
                // overstates nothing, and saying "stale" about it would be
                // wrong.
                if (effective < cycleDate) StaleSource(credit.name, effective) else null
            }
            .sortedBy { it.name }
    }

    /** One line for the cycle card, or null when everything matches. */
    fun notice(credits: List<DataSourceCredit>, cycleDate: String?): String? {
        val stale = staleSources(credits, cycleDate)
        if (stale.isEmpty()) return null
        val listed = stale.joinToString("; ") { "${it.name} effective ${it.effectiveDate}" }
        return "Some data is from an earlier cycle — $listed"
    }
}
