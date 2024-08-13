import React from "react";
import { BrowserRouter as Router, Route, Routes } from "react-router-dom";

import Header from "./components/Header";
import { APP_DETAILS_PATH, PUBLISH_PATH, STORE_PATH } from "./constants/path";

import StorePage from "./pages/StorePage";
import AppPage from "./pages/AppPage";
import PublishPage from "./pages/PublishPage";
import Testing from "./pages/Testing";


const BASE_URL = import.meta.env.BASE_URL;
if (window.our) window.our.process = BASE_URL?.replace("/", "");

function App() {

  return (
    <div>
      <Router basename={BASE_URL}>
        <Header />
        <Routes>
          <Route path="/testing" element={<Testing />} />
          <Route path={STORE_PATH} element={<StorePage />} />
          <Route path={`${APP_DETAILS_PATH}/:id`} element={<AppPage />} />
          <Route path={PUBLISH_PATH} element={<PublishPage />} />
        </Routes>
      </Router>
    </div >
  );
}

export default App;
