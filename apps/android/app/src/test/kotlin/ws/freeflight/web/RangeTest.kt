package ws.freeflight.web

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** PMTiles reads charts by byte range; a wrong slice is a garbled map. */
class RangeTest {
    private fun parse(header: String, total: Long = 1000) = AppWebViewClient.parseRange(header, total)

    @Test fun closedRange() = assertEquals(0L to 127L, parse("bytes=0-127"))
    @Test fun openEndedRunsToTheEnd() = assertEquals(900L to 999L, parse("bytes=900-"))
    @Test fun suffixIsTheLastNBytes() = assertEquals(990L to 999L, parse("bytes=-10"))
    @Test fun endPastTheFileIsClamped() = assertEquals(500L to 999L, parse("bytes=500-5000"))
    @Test fun startPastTheFileIsUnsatisfiable() = assertNull(parse("bytes=1000-1001"))
    @Test fun garbageIsIgnored() = assertNull(parse("bytes=abc"))
}
