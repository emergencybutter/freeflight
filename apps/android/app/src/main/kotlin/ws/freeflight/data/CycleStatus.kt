package ws.freeflight.data

import uniffi.ff_uniffi.CycleCurrency
import java.time.LocalDate

/** What to say about a cycle's date beyond the date itself. */
data class CycleStatus(
    val currency: CycleCurrency,
    val daysUntilEffective: Long?,
) {
    /** A line for the cycle card, or null when the date speaks for itself. */
    val notice: String?
        get() = when (currency) {
            CycleCurrency.NOT_YET_EFFECTIVE -> daysUntilEffective?.let {
                val days = if (it == 1L) "1 day" else "$it days"
                "Not in effect for another $days — procedures in it are not current yet"
            } ?: "Not in effect yet"
            CycleCurrency.EXPIRED -> "Out of date — a newer cycle has taken effect"
            CycleCurrency.CURRENT -> null
        }
}

/**
 * Whether the installed cycle is pre-loaded, in force, or superseded.
 *
 * The FAA publishes a cycle before the day it takes effect so clients can
 * pre-load it, and this app happily downloads one — as it should. What it
 * could not do until now is *say* so: the cycle card and the map chip
 * both read "Effective 2026-10-01" whether that is two weeks away or six
 * weeks past, and a pilot reading procedures from a cycle that has not
 * started is reading procedures that are not yet legal.
 *
 * Showing a date is only explicit about freshness (DESIGN.md §11) if the
 * reader can tell how it relates to today. The classification lives in
 * `ff_core::cycle` so the web client can make the same statement.
 */
object CycleStatusReader {

    fun of(effectiveDate: String?, today: LocalDate = LocalDate.now()): CycleStatus? {
        val effective = effectiveDate?.takeIf { it.isNotBlank() } ?: return null
        val iso = today.toString()
        val currency = uniffi.ff_uniffi.cycleCurrency(effective, iso) ?: return null
        return CycleStatus(
            currency = currency,
            daysUntilEffective = uniffi.ff_uniffi.daysUntilEffective(effective, iso),
        )
    }
}
