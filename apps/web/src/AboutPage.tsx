import { useEffect, useState } from "react";
import { fetchJson } from "./api";
import "./AboutPage.css";

/** One row of `/data/attributions` — a data source shipped in the current
 * cycle bundle, with its effective date and required credit. */
interface DataSource {
  name: string;
  effective_date: string | null;
  licence: string | null;
  url: string | null;
  attribution: string;
}

/** Static `/about` route — see main.tsx for the path check that renders
 * this instead of the map app. No router: freeflight has none, and a
 * single static page doesn't need one. It fetches `/data/attributions` so
 * the France/SIA credit can show the cycle's actual effective date, which
 * the Licence Ouverte requires. */
export function AboutPage() {
  const [sources, setSources] = useState<DataSource[]>([]);
  useEffect(() => {
    fetchJson<DataSource[]>("/data/attributions")
      .then(setSources)
      .catch(() => setSources([]));
  }, []);
  const sia = sources.find((s) => s.name.includes("SIA"));

  return (
    <div className="about-page">
      <div className="about-card">
        <h1>freeflight</h1>
        <p>
          freeflight is a free electronic flight bag: current FAA VFR/IFR charts, airport and
          instrument-procedure data, live METAR/TAF/NOTAM/AIRMET-SIGMET weather, and simple route
          planning, all in one fast map.
        </p>
        <p>
          It's a situational-awareness and planning tool, not certified navigation equipment —
          always cross-check against official sources before you fly.
        </p>
        <p>
          freeflight is built by <a href="https://flyvoyager.net">flyvoyager.net</a>. Bug reports,
          feature requests, and general feedback are welcome on our{" "}
          <a href="https://flyvoyager.net/discord">Discord</a>.
        </p>

        <h2>Data sources</h2>
        <p>freeflight is built entirely on free, official aeronautical data:</p>
        <ul className="about-sources">
          <li>
            US charts, airport and instrument-procedure data, and airspace —{" "}
            <a href="https://www.faa.gov/">Federal Aviation Administration</a> (public domain).
          </li>
          <li>
            Weather (METAR/TAF, AIRMET/SIGMET, winds aloft) —{" "}
            <a href="https://aviationweather.gov/">NOAA Aviation Weather Center</a>.
          </li>
          <li>
            Non-US aeronautical data, where included, comes from each country's official AIS. French
            data: © Service de l'Information Aéronautique (SIA), reused under the{" "}
            <a href="https://www.etalab.gouv.fr/licence-ouverte-open-licence/">Licence Ouverte</a>,
            sourced from{" "}
            <a href="https://www.sia.aviation-civile.gouv.fr">sia.aviation-civile.gouv.fr</a>
            {sia?.effective_date ? `, AIRAC effective ${sia.effective_date}.` : " and updated each AIRAC cycle."}
          </li>
        </ul>

        <a className="about-back" href="/">
          ← Back to the app
        </a>
      </div>
    </div>
  );
}
