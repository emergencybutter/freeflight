use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChartKind {
    Sectional,
    TerminalAreaChart,
    /// VFR Flyway Planning chart — printed on the reverse of a TAC and
    /// shipped as its own `<City> FLY.tif` inside the same TAC zip.
    VfrFlyway,
    WorldAeronauticalChart,
    IfrEnrouteLow,
    IfrEnrouteHigh,
    HelicopterRoute,
    /// FAA d-TPP approach/SID/STAR plate (source PDF, not a tiled raster).
    TerminalProcedurePlate,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BoundingBox {
    pub min_lat: f64,
    pub min_lon: f64,
    pub max_lat: f64,
    pub max_lon: f64,
}

impl BoundingBox {
    pub fn contains(&self, lat: f64, lon: f64) -> bool {
        lat >= self.min_lat && lat <= self.max_lat && lon >= self.min_lon && lon <= self.max_lon
    }

    pub fn intersects(&self, other: &BoundingBox) -> bool {
        self.min_lat <= other.max_lat
            && self.max_lat >= other.min_lat
            && self.min_lon <= other.max_lon
            && self.max_lon >= other.min_lon
    }
}

/// One entry in the published chart catalog (DESIGN.md §6 `chart_catalog`
/// table) — metadata about a tiled chart product, pointing at where its
/// PMTiles archive (or, for plates, source PDF) lives.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChartCatalogEntry {
    pub id: String,
    pub name: String,
    pub kind: ChartKind,
    pub cycle_id: String,
    pub bbox: BoundingBox,
    pub tile_url: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChartCatalog {
    pub entries: Vec<ChartCatalogEntry>,
}

impl ChartCatalog {
    pub fn covering(&self, lat: f64, lon: f64) -> Vec<&ChartCatalogEntry> {
        self.entries
            .iter()
            .filter(|e| e.bbox.contains(lat, lon))
            .collect()
    }

    pub fn of_kind(&self, kind: ChartKind) -> Vec<&ChartCatalogEntry> {
        self.entries.iter().filter(|e| e.kind == kind).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entry() -> ChartCatalogEntry {
        ChartCatalogEntry {
            id: "sfo-sectional".into(),
            name: "San Francisco Sectional".into(),
            kind: ChartKind::Sectional,
            cycle_id: "2026-07".into(),
            bbox: BoundingBox {
                min_lat: 35.0,
                min_lon: -124.0,
                max_lat: 39.0,
                max_lon: -120.0,
            },
            tile_url: "https://cdn.example/charts/sfo-sectional.pmtiles".into(),
        }
    }

    #[test]
    fn finds_chart_covering_a_point() {
        let catalog = ChartCatalog {
            entries: vec![sample_entry()],
        };
        assert_eq!(catalog.covering(37.6, -122.4).len(), 1);
        assert_eq!(catalog.covering(0.0, 0.0).len(), 0);
    }
}
