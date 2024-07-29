import React from "react";
import { useParams } from "react-router-dom";
import { FaDownload, FaSync, FaTrash, FaMagnet, FaCog, FaCheck, FaTimes } from "react-icons/fa";
import useAppsStore from "../store";
import { appId } from "../utils/app";

export default function AppPage() {
  const { installApp, updateApp, uninstallApp, setMirroring, setAutoUpdate, apps } = useAppsStore();
  const { id } = useParams();
  const app = apps.find(a => appId(a) === id);

  if (!app) {
    return <div className="app-page"><h4>App details not found for {id}</h4></div>;
  }

  const handleInstall = () => app && installApp(app);
  const handleUpdate = () => app && updateApp(app);
  const handleUninstall = () => app && uninstallApp(app);
  const handleMirror = () => app && setMirroring(app, !app.state?.mirroring);
  const handleAutoUpdate = () => app && setAutoUpdate(app, !app.state?.auto_update);

  return (
    <section className="app-page">
      <h2>{app.metadata?.name || app.package}</h2>
      <p className="app-description">{app.metadata?.description || "No description available"}</p>
      <div className="app-details">
        <div className="app-info">
          <ul className="app-details-list">
            <li><span>Version:</span> <span>{app.metadata?.properties?.current_version || "Unknown"}</span></li>
            <li><span>Developer:</span> <span>{app.publisher}</span></li>
            <li><span>Mirrors:</span> <span>{app.metadata?.properties?.mirrors?.length || 0}</span></li>
            <li>
              <span>Installed:</span>
              {app.installed ? <FaCheck className="status-icon installed" /> : <FaTimes className="status-icon not-installed" />}
            </li>
            <li>
              <span>Mirroring:</span>
              {app.state?.mirroring ? <FaCheck className="status-icon mirroring" /> : <FaTimes className="status-icon not-mirroring" />}
            </li>
            <li>
              <span>Auto-Update:</span>
              {app.state?.auto_update ? <FaCheck className="status-icon auto-update" /> : <FaTimes className="status-icon no-auto-update" />}
            </li>
          </ul>
        </div>
        <div className="app-actions">
          {app.installed ? (
            <>
              <button onClick={handleUpdate} className="secondary"><FaSync /> Update</button>
              <button onClick={handleUninstall} className="secondary"><FaTrash /> Uninstall</button>
            </>
          ) : (
            <button onClick={handleInstall}><FaDownload /> Install</button>
          )}
          <button onClick={handleMirror} className="secondary">
            <FaMagnet /> {app.state?.mirroring ? "Stop Mirroring" : "Start Mirroring"}
          </button>
          <button onClick={handleAutoUpdate} className="secondary">
            <FaCog /> {app.state?.auto_update ? "Disable Auto-Update" : "Enable Auto-Update"}
          </button>
        </div>
      </div>
      {app.metadata?.properties?.screenshots && (
        <div className="app-screenshots">
          {app.metadata.properties.screenshots.map((screenshot, index) => (
            <img key={index} src={screenshot} alt={`Screenshot ${index + 1}`} className="app-screenshot" />
          ))}
        </div>
      )}
    </section>
  );
}