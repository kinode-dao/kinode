import React, { useState, useEffect, useMemo, useCallback } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { FaGlobe, FaPeopleGroup } from "react-icons/fa6";

import { AppInfo } from "../types/Apps";
import useAppsStore from "../store/apps-store";
import ActionButton from "../components/ActionButton";
import AppHeader from "../components/AppHeader";
import SearchHeader from "../components/SearchHeader";
import { appId } from "../utils/app";
import { PUBLISH_PATH } from "../constants/path";
import HomeButton from "../components/HomeButton";

export default function AppPage() {
  const { myApps, listedApps, getListedApp } = useAppsStore();
  const navigate = useNavigate();
  const params = useParams();
  const [app, setApp] = useState<AppInfo | undefined>(undefined);
  const [launchPath, setLaunchPath] = useState('');

  useEffect(() => {
    const myApp = myApps.local.find((a) => appId(a) === params.id);
    if (myApp) return setApp(myApp);

    if (params.id) {
      const app = listedApps.find((a) => appId(a) === params.id);
      if (app) {
        setApp(app);
      } else {
        getListedApp(params.id)
          .then((app) => setApp(app))
          .catch(console.error);
      }
    }
  }, [params.id, myApps, listedApps, getListedApp]);

  const goToPublish = useCallback(() => {
    navigate(PUBLISH_PATH, { state: { app } });
  }, [app, navigate]);

  const version = useMemo(
    () => app?.metadata?.properties?.current_version || "Unknown",
    [app]
  );
  const versions = Object.entries(app?.metadata?.properties?.code_hashes || {});
  const hash =
    app?.state?.our_version ||
    (versions[(versions.length || 1) - 1] || ["", ""])[1];

  const appDetails = [
    {
      top: "Developer",
      middle: <FaPeopleGroup size={36} />,
      bottom: app?.publisher
    },
    {
      top: "Version",
      middle: <span className="version-number">{version}</span>,
      bottom: `${hash.slice(0, 5)}...${hash.slice(-5)}`
    },
    {
      top: "Mirrors",
      middle: <FaGlobe size={36} />,
      bottom: app?.metadata?.properties?.mirrors?.length || 0
    }
  ];

  useEffect(() => {
    fetch('/apps').then(data => data.json())
      .then((data: Array<{ package_name: string, path: string }>) => {
        if (Array.isArray(data)) {
          const homepageAppData = data.find(otherApp => app?.package === otherApp.package_name)
          if (homepageAppData) {
            setLaunchPath(homepageAppData.path)
          }
        }
      })
  }, [app])

  return (
    <div className="app-page">
      <HomeButton />
      <SearchHeader
        value=""
        onChange={() => null}
        hideSearch
        hidePublish
      />
      <div className="app-details-card">
        {app ? (
          <>
            <AppHeader app={app} size="large" />
            <hr className="app-divider" />
            <div className="app-info-grid">
              {appDetails.map((detail, index) => (
                <React.Fragment key={index}>
                  <div className="app-info-item">
                    <div className="app-info-top">{detail.top}</div>
                    <div className="app-info-middle">{detail.middle}</div>
                    <div className="app-info-bottom">{detail.bottom}</div>
                  </div>
                  {index !== appDetails.length - 1 && <div className="app-info-divider" />}
                </React.Fragment>
              ))}
            </div>
            {Array.isArray(app.metadata?.properties?.screenshots) && app.metadata?.properties.screenshots.length > 0 && (
              <div className="app-screenshots">
                {app.metadata.properties.screenshots.map((screenshot, index) => (
                  <img key={index + screenshot} src={screenshot} alt={`Screenshot ${index + 1}`} className="app-screenshot" />
                ))}
              </div>
            )}
            <div className="app-actions">
              <ActionButton
                app={app}
                launchPath={launchPath}
                className="app-action-button"
                permitMultiButton
              />
            </div>
            {app.installed && app.state?.mirroring && (
              <button type="button" onClick={goToPublish} className="publish-button">
                Publish
              </button>
            )}
          </>
        ) : (
          <>
            <h4>App details not found for</h4>
            <h4>{params.id}</h4>
          </>
        )}
      </div>
    </div>
  );
}