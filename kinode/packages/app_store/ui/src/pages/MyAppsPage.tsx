import React, { useState, useEffect, useCallback } from "react";
import { FaUpload } from "react-icons/fa";

import { AppInfo, MyApps } from "../types/Apps";
import useAppsStore from "../store/apps-store";
import AppEntry from "../components/AppEntry";
import SearchHeader from "../components/SearchHeader";
import { PageProps } from "../types/Page";
import { useNavigate } from "react-router-dom";
import { appId } from "../utils/app";
import { PUBLISH_PATH } from "../constants/path";
import HomeButton from "../components/HomeButton";
import { isMobileCheck } from "../utils/dimensions";
import classNames from "classnames";

export default function MyAppsPage() { // eslint-disable-line
  const { myApps, getMyApps, } = useAppsStore()
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

  const isMobile = isMobileCheck()
  console.log({ myApps })

  return (
    <div className={classNames("flex flex-col w-full h-screen p-2",
      {
        'gap-4 max-w-screen': isMobile,
        'gap-8 max-w-[900px]': !isMobile,
      })}>
      {!isMobile && <HomeButton />}
      <SearchHeader value={searchQuery} onChange={searchMyApps} />
      <div className="flex justify-between items-center mt-2">
        <h3>My Packages</h3>
        <button className="alt" onClick={() => navigate(PUBLISH_PATH)}>
          <FaUpload className="mr-2" />
          Publish Package
        </button>
      </div>

      <div className={classNames("flex flex-col card gap-2 mt-2",
        {
          'max-h-[80vh] overflow-y-scroll overflow-x-visible': !isMobile,
        })}
        style={{
          scrollbarWidth: 'thin',
          scrollbarColor: '#FFF5D9 transparent',
        }}
      >
        {displayedApps.downloaded.length > 0 && <h4>Downloaded</h4>}
        {(displayedApps.downloaded || []).map((app) => <AppEntry
          key={appId(app)}
          app={app}
          showMoreActions
        />)}
        {displayedApps.installed.length > 0 && <h4>Installed</h4>}
        {(displayedApps.installed || []).map((app) => <AppEntry
          key={appId(app)}
          app={app}
          showMoreActions
        />)}
        {displayedApps.local.length > 0 && <h4>Local</h4>}
        {(displayedApps.local || []).map((app) => <AppEntry
          key={appId(app)}
          app={app}
          showMoreActions
        />)}
        {displayedApps.system.length > 0 && <h4>System</h4>}
        {(displayedApps.system || []).map((app) => <AppEntry
          key={appId(app)}
          app={app}
          showMoreActions
        />)}
      </div>
    </div>
  );
}
