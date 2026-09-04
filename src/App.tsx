import { useCallback, useRef, useState } from "react";

import "./App.css";
import Dock, { DockHandle, PanelKind } from "./dock/Dock";
import { Rail } from "./dock/Rail";
import { TaskBar } from "./dock/TaskBar";
import { LibraryView } from "./components/library/LibraryView";
import { MillerView } from "./components/viewer/MillerView";

function App() {
  const dockRef = useRef<DockHandle | null>(null);
  const [activeKind, setActiveKind] = useState<PanelKind | null>("library");

  const renderPanel = useCallback((kind: PanelKind, isActive: boolean) => {
    switch (kind) {
      case "library":
        return <LibraryView mode="library" isActive={isActive} />;
      case "scattered":
        return <LibraryView mode="scattered" isActive={isActive} />;
      case "viewer":
        return <MillerView isActive={isActive} />;
    }
  }, []);

  return (
    <div className="shell">
      <Rail activeKind={activeKind} onOpen={(kind, title) => dockRef.current?.open(kind, title)} />
      <div className="main">
        <div className="dock-area">
          <Dock
            renderPanel={renderPanel}
            onActivePanelChange={(_title, kind) => setActiveKind(kind)}
            onReady={(handle) => (dockRef.current = handle)}
          />
        </div>
        <TaskBar />
      </div>
    </div>
  );
}

export default App;
