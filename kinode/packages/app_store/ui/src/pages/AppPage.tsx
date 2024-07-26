import React, { useState, useEffect } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { FaGlobe, FaPeopleGroup, FaCode } from "react-icons/fa6";

import { AppInfo } from "../types/Apps";
import useAppsStore from "../store";
import { PUBLISH_PATH } from "../constants/path";

export default function AppPage() {
  const { getApp, installApp, updateApp, uninstallApp, setMirroring, setAutoUpdate } = useAppsStore();
  const navigate = useNavigate();
  const { id } = useParams();
  const [app, setApp] = useState<AppInfo | undefined>(undefined);
  const [launchPath, setLaunchPath] = useState('');

  useEffect(() => {
    if (id) {
      getApp(id).then(setApp).catch(console.error);
    }
  }, [id, getApp]);

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

  const handleInstall = () => app && installApp(app);
  const handleUpdate = () => app && updateApp(app);
  const handleUninstall = () => app && uninstallApp(app);
  const handleMirror = () => app && setMirroring(app, !app.state?.mirroring);
  const handleAutoUpdate = () => app && setAutoUpdate(app, !app.state?.auto_update);
  const handlePublish = () => navigate(PUBLISH_PATH, { state: { app } });

  if (!app) {
    return <div className="app-page"><h4>App details not found for {id}</h4></div>;
  }

  const version = app.metadata?.properties?.current_version || "Unknown";
  const versions = Object.entries(app.metadata?.properties?.code_hashes || {});
  const hash = app.state?.our_version || (versions[(versions.length || 1) - 1] || ["", ""])[1];
  const mirrors = app.metadata?.properties?.mirrors?.length || 0;

  return (
    <div className="app-page">
      <div className="app-details-card">
        <h2>{app.metadata?.name || app.package}</h2>
        <p>{app.metadata?.description || "No description available"}</p>
        <hr className="app-divider" />
        <div className="app-info-grid">
          <div className="app-info-item">
            <FaPeopleGroup size={36} />
            <span>Developer</span>
            <span>{app.publisher}</span>
          </div>
          <div className="app-info-item">
            <FaCode size={36} />
            <span>Version</span>
            <span>{version}</span>
            <span className="hash">{`${hash.slice(0, 5)}...${hash.slice(-5)}`}</span>
          </div>
          <div className="app-info-item">
            <FaGlobe size={36} />
            <span>Mirrors</span>
            <span>{mirrors}</span>
          </div>
        </div>
        {app.metadata?.properties?.screenshots && (
          <div className="app-screenshots">
            {app.metadata.properties.screenshots.map((screenshot, index) => (
              <img key={index} src={screenshot} alt={`Screenshot ${index + 1}`} className="app-screenshot" />
            ))}
          </div>
        )}
        <div className="app-actions">
          {app.installed ? (
            <>
              <button onClick={handleUpdate}>Update</button>
              <button onClick={handleUninstall}>Uninstall</button>
              <button onClick={handleMirror}>
                {app.state?.mirroring ? "Stop Mirroring" : "Start Mirroring"}
              </button>
              <button onClick={handleAutoUpdate}>
                {app.state?.auto_update ? "Disable Auto-Update" : "Enable Auto-Update"}
              </button>
              {app.state?.mirroring && (
                <button onClick={handlePublish}>Publish</button>
              )}
            </>
          ) : (
            <button onClick={handleInstall}>Install</button>
          )}
          {launchPath && (
            <a href={launchPath} className="button">Launch</a>
          )}
        </div>
      </div>
    </div>
  );
}