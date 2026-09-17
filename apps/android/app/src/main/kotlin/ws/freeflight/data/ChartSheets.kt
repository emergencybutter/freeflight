package ws.freeflight.data

import uniffi.ff_uniffi.Chart

/**
 * What one chart sheet is called in the UI, and the order sheets appear in.
 *
 * Delegates to `crates/ff-core/src/vocabulary.rs`, the same definitions
 * web reads. Most kinds keep the catalogue's own name — "Albuquerque
 * Sectional" needs no help — and only the IFR enroute series is renamed,
 * because the catalogue names those after the FAA's file stem
 * (`enr_l06n`), which identifies nothing to a pilot.
 *
 * The core derives the label from the chart *id*, not the name: a panel
 * shipping several parts gave every part the panel's name until `ff-etl`
 * was fixed, so two different downloads shared one label. Cycles built
 * before that fix are still on devices, and their ids were always right.
 */
object ChartSheets {

    fun label(chart: Chart): String =
        uniffi.ff_uniffi.chartSheetLabel(chart.id, chart.name)

    fun sortKey(chart: Chart): String =
        uniffi.ff_uniffi.chartSheetSortKey(chart.id, chart.name)
}
