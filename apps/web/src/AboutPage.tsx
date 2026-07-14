import "./AboutPage.css";

/** Static `/about` route — see main.tsx for the path check that renders
 * this instead of the map app. No router: freeflight has none, and a
 * single static page doesn't need one. */
export function AboutPage() {
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
        <a className="about-back" href="/">
          ← Back to the app
        </a>
      </div>
    </div>
  );
}
