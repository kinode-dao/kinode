import React from "react";
import { BrowserRouter as Router, Route, Routes } from "react-router-dom";

import StorePage from "./pages/StorePage";
import MyAppsPage from "./pages/MyAppsPage";
import AppPage from "./pages/AppPage";
import { APP_DETAILS_PATH, MY_APPS_PATH, PUBLISH_PATH, STORE_PATH } from "./constants/path";
import PublishPage from "./pages/PublishPage";
import Header from "./components/Header";

const BASE_URL = import.meta.env.BASE_URL;
if (window.our) window.our.process = BASE_URL?.replace("/", "");

function App() {

  return (
    <div>
      <Router basename={BASE_URL}>
        <Header />
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
