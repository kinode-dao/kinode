import React, { useState } from "react";
import { BrowserRouter as Router, Route, Routes } from "react-router-dom";

import StorePage from "./pages/StorePage";
import MyAppsPage from "./pages/MyAppsPage";
import AppPage from "./pages/AppPage";
import { APP_DETAILS_PATH, MY_APPS_PATH, PUBLISH_PATH, STORE_PATH } from "./constants/path";
import PublishPage from "./pages/PublishPage";


const BASE_URL = import.meta.env.BASE_URL;
if (window.our) window.our.process = BASE_URL?.replace("/", "");

const PROXY_TARGET = `${import.meta.env.VITE_NODE_URL || "http://localhost:8080"
  }${BASE_URL}`;

// This env also has BASE_URL which should match the process + package name
const WEBSOCKET_URL = import.meta.env.DEV // eslint-disable-line
  ? `${PROXY_TARGET.replace("http", "ws")}`
  : undefined;

function App() {
  const [nodeConnected, setNodeConnected] = useState(true); // eslint-disable-line

  if (!nodeConnected) {
    return (
      <div className="flex flex-col c">
        <h2 style={{ color: "red" }}>Node not connected</h2>
        <h4>
          You need to start a node at {PROXY_TARGET} before you can use this UI
          in development.
        </h4>
      </div>
    );
  }

  return (
    <div className="flex flex-col c h-screen w-screen max-h-screen max-w-screen overflow-x-hidden special-appstore-background">
      <Router basename={BASE_URL}>
        <Routes>
          <Route path={STORE_PATH} element={<StorePage />} />
          <Route path={MY_APPS_PATH} element={<MyAppsPage />} />
          <Route path={`${APP_DETAILS_PATH}/:id`} element={<AppPage />} />
          <Route path={PUBLISH_PATH} element={<PublishPage />} />
        </Routes>
      </Router>
    </div >
  );
}

export default App;
