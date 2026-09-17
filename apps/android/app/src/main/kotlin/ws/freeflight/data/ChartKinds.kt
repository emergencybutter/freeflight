package ws.freeflight.data

/**
 * Labels and ordering for `chart_catalog.kind`.
 *
 * The definitions live in Rust (`crates/ff-core/src/vocabulary.rs`) and
 * this delegates to them through `ff-uniffi`. They used to be duplicated
 * here and in `apps/web/src/MapView.tsx`, which is how the two clients
 * came to disagree about what a chart kind is called — someone who knew
 * "IFR Low" on the web should not meet "IfrEnrouteLow" on the phone.
 *
 * Web reads the same definitions from a generated TypeScript module
 * rather than through wasm; the map path loads no wasm, and a label is
 * not worth an async init on that screen.
 *
 * Kept as a Kotlin object rather than calling the bindings directly at
 * every call site, so the UI keeps a small idiomatic surface and the
 * binding stays one hop away.
 */
object ChartKinds {

    /** The kind a fresh install draws, when it has one. */
    val DEFAULT: String get() = uniffi.ff_uniffi.defaultChartKind()

    fun label(kind: String): String = uniffi.ff_uniffi.chartKindLabel(kind)

    fun order(kind: String): Int = uniffi.ff_uniffi.chartKindOrder(kind).toInt()

    /** Kinds present in `kinds`, in the order the selector should list them. */
    fun ordered(kinds: Collection<String>): List<String> =
        kinds.distinct().sortedWith(compareBy({ order(it) }, { it }))
}
