import React, { useState, useEffect, useCallback } from "react";
import { FaUpload } from "react-icons/fa";

import { AppInfo, MyApps } from "../types/Apps";
import useAppsStore from "../store/apps-store";
import AppEntry from "../components/AppEntry";
import SearchHeader from "../components/SearchHeader";
import { PageProps } from "../types/Page";
import { useNavigate } from "react-router-dom";
import { appId } from "../utils/app";

interface MyAppsPageProps extends PageProps {}

export default function MyAppsPage(props: MyAppsPageProps) { // eslint-disable-line
  const { myApps, getMyApps } = useAppsStore()
  const navigate = useNavigate();

  const [searchQuery, setSearchQuery] = useState<string>("");
  const [displayedApps, setDisplayedApps] = useState<MyApps>(myApps);

  useEffect(() => {
    getMyApps()
      .then(setDisplayedApps)
      .catch((error) => console.error(error));
  }, []); // eslint-disable-line

  const searchMyApps = useCallback((query: string) => {
    setSearchQuery(query);
    const filteredApps = Object.keys(myApps).reduce((acc, key) => {
      acc[key] = myApps[key].filter((app) => {
        return app.package.toLowerCase().includes(query.toLowerCase())
          || app.metadata?.description?.toLowerCase().includes(query.toLowerCase())
          || app.metadata?.description?.toLowerCase().includes(query.toLowerCase());
      })

      return acc
    }, {
      downloaded: [] as AppInfo[],
      installed: [] as AppInfo[],
      local: [] as AppInfo[],
      system: [] as AppInfo[],
    } as MyApps)

    setDisplayedApps(filteredApps);
  }, [myApps]);

  useEffect(() => {
    if (searchQuery) {
      searchMyApps(searchQuery);
    } else {
      setDisplayedApps(myApps);
    }
  }, [myApps]);

  return (
    <div style={{ width: "100%", height: '100%' }}>
      <SearchHeader value={searchQuery} onChange={searchMyApps} />
      <div className="row between page-title">
        <h4 style={{ marginBottom: "0.5em" }}>My Packages</h4>
        <button className="row" onClick={() => navigate('/publish')}>
          <FaUpload style={{ marginRight: "0.5em" }} />
          Publish Package
        </button>
      </div>

      <div className="my-apps-list">
        <div className="new card col" style={{ gap: "1em" }}>
          <h4>Downloaded</h4>
          {(displayedApps.downloaded || []).map((app) => <AppEntry key={appId(app)} app={app} />)}
          <h4>Installed</h4>
          {(displayedApps.installed || []).map((app) => <AppEntry key={appId(app)} app={app} />)}
          <h4>Local</h4>
          {(displayedApps.local || []).map((app) => <AppEntry key={appId(app)} app={app} />)}
          <h4>System</h4>
          {(displayedApps.system || []).map((app) => <AppEntry key={appId(app)} app={app} />)}
        </div>
      </div>
    </div>
  );
}
