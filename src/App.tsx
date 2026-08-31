import "./App.css";
import { LibraryView } from "./components/library/LibraryView";

// Every future pane is a different projection over the same store (see the
// architecture note). Only Library reads one today; the rest are named here
// so the shell's shape doesn't change as they land — each just stops being
// disabled.
const VIEWS = [
  "Library",
  "Scattered",
  "Miller",
  "Grid",
  "Board",
  "Note",
  "Inspector",
  "Graph",
  "Discover",
  "Compass",
] as const;

function App() {
  const active: (typeof VIEWS)[number] = "Library";

  return (
    <div className="shell">
      <header className="top">
        <span className="mark">Archiva</span>
        <nav className="nav" aria-label="Views">
          {VIEWS.map((v) => (
            <button key={v} className={v === active ? "active" : ""} disabled={v !== active}>
              {v}
            </button>
          ))}
        </nav>
      </header>
      <LibraryView />
    </div>
  );
}

export default App;
