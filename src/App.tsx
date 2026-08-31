import { useCallback, useRef, useState } from "react";

import "./App.css";
import Dock, { DockHandle, PanelKind } from "./dock/Dock";
import { Rail } from "./dock/Rail";
import { LibraryView, LibraryViewHandle } from "./components/library/LibraryView";

function App() {
  const dockRef = useRef<DockHandle | null>(null);
  const libraryRef = useRef<LibraryViewHandle | null>(null);
  const [activePanel, setActivePanel] = useState<string | null>(null);

  const renderPanel = useCallback((kind: PanelKind) => {
    switch (kind) {
      case "library":
        return <LibraryView ref={libraryRef} />;
    }
  }, []);

  return (
    <div className="shell">
      <Rail
        active="library"
        onSelect={(kind) => dockRef.current?.open(kind as PanelKind, "Library")}
        onAddFolder={() => libraryRef.current?.addFolder()}
      />
      <div className="dock-area" aria-label={activePanel ?? undefined}>
        <Dock
          renderPanel={renderPanel}
          onActivePanelChange={setActivePanel}
          onReady={(handle) => (dockRef.current = handle)}
        />
      </div>
    </div>
  );
}

export default App;
