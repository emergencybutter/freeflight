package ws.freeflight.data

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * The selector is driven entirely by these two functions, so the cases
 * that matter are the ones where a cycle publishes something this table
 * has never seen. A kind that vanished from the menu would be a chart the
 * pilot downloaded and then could not draw.
 */
class ChartKindsTest {

    @Test
    fun `known kinds get the same short labels the web client uses`() {
        assertEquals("Sectional", ChartKinds.label("Sectional"))
        assertEquals("TAC", ChartKinds.label("TerminalAreaChart"))
        assertEquals("IFR Low", ChartKinds.label("IfrEnrouteLow"))
    }

    @Test
    fun `an unknown kind falls back to its raw name rather than disappearing`() {
        assertEquals("GrandCanyonVfr", ChartKinds.label("GrandCanyonVfr"))
        assertEquals(listOf("Sectional", "GrandCanyonVfr"), ChartKinds.ordered(listOf("GrandCanyonVfr", "Sectional")))
    }

    @Test
    fun `ordering runs VFR broad to terminal, then IFR, then specialty`() {
        val shuffled = listOf("HelicopterRoute", "IfrEnrouteHigh", "TerminalAreaChart", "Sectional", "IfrEnrouteLow", "VfrFlyway")
        assertEquals(
            listOf("Sectional", "TerminalAreaChart", "VfrFlyway", "IfrEnrouteLow", "IfrEnrouteHigh", "HelicopterRoute"),
            ChartKinds.ordered(shuffled),
        )
    }

    @Test
    fun `unknown kinds sort among themselves by name, never interleaved with known ones`() {
        assertEquals(
            listOf("Sectional", "Alpha", "Beta"),
            ChartKinds.ordered(listOf("Beta", "Alpha", "Sectional")),
        )
    }

    @Test
    fun `a kind listed twice is offered once`() {
        assertEquals(listOf("Sectional"), ChartKinds.ordered(listOf("Sectional", "Sectional")))
    }

    @Test
    fun `the default kind is one the table knows how to label`() {
        assertEquals("Sectional", ChartKinds.label(ChartKinds.DEFAULT))
    }
}
