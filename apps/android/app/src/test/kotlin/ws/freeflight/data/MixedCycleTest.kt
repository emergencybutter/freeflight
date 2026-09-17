package ws.freeflight.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import uniffi.ff_uniffi.DataSourceCredit

class MixedCycleTest {

    private fun credit(name: String, effective: String?) = DataSourceCredit(
        name = name,
        effectiveDate = effective,
        licence = null,
        url = null,
        attribution = name,
    )

    private val faa = credit("FAA", "2026-10-01")

    @Test
    fun `a cycle whose sources all match its date has nothing to say`() {
        val credits = listOf(faa, credit("France (SIA)", "2026-10-01"))
        assertNull(MixedCycle.notice(credits, "2026-10-01"))
    }

    @Test
    fun `a source from an earlier cycle is named with its own date`() {
        val credits = listOf(faa, credit("France (SIA)", "2026-09-03"))
        assertEquals(
            listOf(StaleSource("France (SIA)", "2026-09-03")),
            MixedCycle.staleSources(credits, "2026-10-01"),
        )
        assertEquals(
            "Some data is from an earlier cycle — France (SIA) effective 2026-09-03",
            MixedCycle.notice(credits, "2026-10-01"),
        )
    }

    @Test
    fun `several stale sources are all listed`() {
        val credits = listOf(faa, credit("openAIP", "2026-08-06"), credit("France (SIA)", "2026-09-03"))
        assertEquals(
            "Some data is from an earlier cycle — France (SIA) effective 2026-09-03; " +
                "openAIP effective 2026-08-06",
            MixedCycle.notice(credits, "2026-10-01"),
        )
    }

    @Test
    fun `a source newer than the cycle is not called stale`() {
        // Overstates nothing, so warning about it would just be wrong.
        val credits = listOf(faa, credit("France (SIA)", "2026-10-29"))
        assertNull(MixedCycle.notice(credits, "2026-10-01"))
    }

    @Test
    fun `an undated source is ignored rather than guessed at`() {
        // ff-etl's validation refuses to publish one, so this is only
        // reachable for a bundle built before that check existed.
        val credits = listOf(faa, credit("France (SIA)", null), credit("openAIP", ""))
        assertNull(MixedCycle.notice(credits, "2026-10-01"))
    }

    @Test
    fun `with no cycle date there is nothing to compare against`() {
        assertNull(MixedCycle.notice(listOf(credit("France (SIA)", "2026-09-03")), null))
    }
}
