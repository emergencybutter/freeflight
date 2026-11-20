//! FAA d-TPP vocabulary shared by the ETL that writes `dtpp_chart` rows and
//! every reader of them (`ff-data-api`, `ff-uniffi`).

/// `dtpp_chart.procedure_ident` for an airport's diagram. d-TPP's `APD`
/// records have no procedure ident of their own, so `ff-etl` stores this
/// sentinel there (see `ff-etl::dtpp`). One definition, so writer and
/// readers cannot drift apart — they did once ("AIRPORT DIAGRAM" vs
/// "AIRPORT_DIAGRAM"), and Android silently never found a diagram.
pub const AIRPORT_DIAGRAM_IDENT: &str = "AIRPORT_DIAGRAM";
