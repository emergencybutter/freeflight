package ws.freeflight.data

import org.junit.Assert.assertEquals
import org.junit.Test
import uniffi.ff_uniffi.BoundingBox
import uniffi.ff_uniffi.Chart

/**
 * The slugs here are the real ones from the 2026-08-06 catalogue, which
 * is where the duplicate names were found.
 */
class ChartSheetsTest {

    private fun chart(id: String, name: String, kind: String = "IfrEnrouteLow") = Chart(
        id = "2026-08-06-$id",
        name = name,
        kind = kind,
        bbox = BoundingBox(0.0, 0.0, 0.0, 0.0),
        minZoom = 0u,
        maxZoom = 0u,
        tileUrl = "",
        installed = false,
        installedBytes = 0uL,
        downloadBytes = null,
    )

    @Test
    fun `an ifr panel is named by its faa designator, not its file stem`() {
        assertEquals("L-1", ChartSheets.label(chart("enr_l01", "IFR Low Altitude Enroute enr_l01")))
        assertEquals("H-12", ChartSheets.label(chart("enr_h12", "IFR High Altitude Enroute enr_h12")))
    }

    @Test
    fun `the halves of a split panel are told apart`() {
        // Both of these carried the name "IFR Low Altitude Enroute enr_l06",
        // so the list showed one label for two different downloads.
        val north = chart("enr_l06n", "IFR Low Altitude Enroute enr_l06")
        val south = chart("enr_l06s", "IFR Low Altitude Enroute enr_l06")
        assertEquals("L-6 North", ChartSheets.label(north))
        assertEquals("L-6 South", ChartSheets.label(south))
    }

    @Test
    fun `an inset is told apart from the panel it belongs to`() {
        val panel = chart("enr_l34", "IFR Low Altitude Enroute enr_l34")
        val inset = chart("enr_l34_bost_inset", "IFR Low Altitude Enroute enr_l34")
        assertEquals("L-34", ChartSheets.label(panel))
        assertEquals("L-34 inset (Boston)", ChartSheets.label(inset))
        assertEquals(
            "L-23 inset (Wilmington)",
            ChartSheets.label(chart("enr_l23_wilm_inset", "IFR Low Altitude Enroute enr_l23")),
        )
    }

    @Test
    fun `an unrecognised inset abbreviation is still distinguishable`() {
        assertEquals(
            "L-9 inset (Xyz)",
            ChartSheets.label(chart("enr_l09_xyz_inset", "IFR Low Altitude Enroute enr_l09")),
        )
    }

    @Test
    fun `every other kind keeps the name the catalogue gives it`() {
        assertEquals(
            "Albuquerque Sectional",
            ChartSheets.label(chart("albuquerque", "Albuquerque Sectional", "Sectional")),
        )
        assertEquals(
            "Boston TAC",
            ChartSheets.label(chart("boston_tac", "Boston TAC", "TerminalAreaChart")),
        )
    }

    @Test
    fun `ifr sheets order by panel number rather than as text`() {
        val sheets = listOf("enr_l10", "enr_l2", "enr_l1", "enr_l21").map {
            chart(it, "IFR Low Altitude Enroute $it")
        }
        assertEquals(
            listOf("L-1", "L-2", "L-10", "L-21"),
            sheets.sortedBy { ChartSheets.sortKey(it) }.map { ChartSheets.label(it) },
        )
    }

    @Test
    fun `a panels parts sort with the panel, not away from it`() {
        val sheets = listOf("enr_l07", "enr_l06s", "enr_l06", "enr_l06n").map {
            chart(it, "IFR Low Altitude Enroute $it")
        }
        assertEquals(
            listOf("L-6", "L-6 North", "L-6 South", "L-7"),
            sheets.sortedBy { ChartSheets.sortKey(it) }.map { ChartSheets.label(it) },
        )
    }
}
