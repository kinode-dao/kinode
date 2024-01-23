import { useState, useEffect } from "react";

const BASE_URL = import.meta.env.BASE_URL;
if (window.our) window.our.process = BASE_URL?.replace("/", "");

const PROXY_TARGET = `${(import.meta.env.VITE_NODE_URL || "http://localhost:8080")}${BASE_URL}`;

function App() {
  const [nodeConnected, setNodeConnected] = useState(false);

  useEffect(() => {
    if (window.our?.node && window.our?.process) {
      setNodeConnected(true);
    } else {
      setNodeConnected(false);
    }
  }, []);

  return (
    <div>
      <header className="flex items-center px-4 h-14 w-full">
        <p>
          ID: <strong>{window.our?.node}</strong>
        </p>
      </header>
      <div className="px-4">
        <h1>Memedeck</h1>
        {!nodeConnected && (
          <div className="node-not-connected">
            <h2 style={{ color: "red" }}>Node not connected</h2>
            <h4>
              You need to start a node at {PROXY_TARGET} before you can use this UI
              in development.
            </h4>
          </div>
        )}
      </div>
    </div>
  );
}

export default App;
