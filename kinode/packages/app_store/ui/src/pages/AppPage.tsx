import React, { useState } from "react";
import { useParams } from "react-router-dom";
import { FaDownload, FaSync, FaTrash, FaMagnet, FaCog, FaCheck, FaTimes, FaRocket, FaLink, FaChevronDown, FaChevronUp, FaShieldAlt } from "react-icons/fa";
import useAppsStore from "../store";
import { appId } from "../utils/app";
import { MirrorCheckFile } from "../types/Apps";

export default function AppPage() {
  const { installApp, updateApp, uninstallApp, approveCaps, setMirroring, setAutoUpdate, checkMirror, apps } = useAppsStore();
  const { id } = useParams();
  const app = apps.find(a => appId(a) === id);
  const [showMetadata, setShowMetadata] = useState(true);
  const [showLocalInfo, setShowLocalInfo] = useState(true);
  const [mirrorStatuses, setMirrorStatuses] = useState<{ [mirror: string]: MirrorCheckFile | null }>({});

  if (!app) {
    return <div className="app-page"><h4>App details not found for {id}</h4></div>;
  }

  const handleInstall = () => app && installApp(app);
  const handleUpdate = () => app && updateApp(app);
  const handleUninstall = () => app && uninstallApp(app);
  const handleMirror = () => app && setMirroring(app, !app.state?.mirroring);
  const handleApproveCaps = () => app && approveCaps(app);
  const handleAutoUpdate = () => app && setAutoUpdate(app, !app.state?.auto_update);
  const handleLaunch = () => {
    console.log("Launching app:", app.package);
    window.open(`/${app.package}:${app.publisher}`, '_blank');
  };


  const handleCheckMirror = (mirror: string) => {
    setMirrorStatuses(prev => ({ ...prev, [mirror]: null })); // Set to loading
    checkMirror(mirror)
      .then(status => setMirrorStatuses(prev => ({ ...prev, [mirror]: status })))
      .catch(error => {
        console.error(`Failed to check mirror ${mirror}:`, error);
        setMirrorStatuses(prev => ({ ...prev, [mirror]: { node: mirror, is_online: false, error: "Request failed" } }));
      });
  };

  return (
    <section className="app-page">
      <div className="app-header">
        {app.metadata?.image && (
          <img src={app.metadata.image} alt={app.metadata?.name || app.package} className="app-icon" />
        )}
        <div className="app-title">
          <h2>{app.metadata?.name || app.package}</h2>
          <p className="app-id">{`${app.package}.${app.publisher}`}</p>
        </div>
      </div>

      <div className="app-description">{app.metadata?.description || "No description available"}</div>

      <div className="app-details">
        <div className="app-info">
          <div className="info-section">
            <h3 onClick={() => setShowMetadata(!showMetadata)}>
              Metadata {showMetadata ? <FaChevronUp /> : <FaChevronDown />}
            </h3>
            {showMetadata && (
              <ul className="detail-list">
                <li><span>Version:</span> <span>{app.metadata?.properties?.current_version || "Unknown"}</span></li>
                <li><span>~metadata-uri</span> <span className="hash">{app.metadata_uri}</span></li>
                <li><span>~metadata-hash</span> <span className="hash">{app.metadata_hash}</span></li>
                <li className="mirrors-list">
                  <span>Mirrors:</span>
                  <ul>
                    {app.metadata?.properties?.mirrors?.map((mirror) => (
                      <li key={mirror} className="mirror-item">
                        <span className="mirror-address">{mirror}</span>
                        <button
                          onClick={() => handleCheckMirror(mirror)}
                          className="check-button"
                          title="Check if mirror is online"
                        >
                          <FaSync className={mirrorStatuses[mirror] === null ? 'spinning' : ''} />
                        </button>
                        {mirrorStatuses[mirror] && (
                          <span className="mirror-status">
                            {mirrorStatuses[mirror]?.is_online ? (
                              <><FaCheck className="online" /> <span className="online">Online</span></>
                            ) : (
                              <>
                                <FaTimes className="offline" />
                                <span className="offline">Offline</span>
                                {mirrorStatuses[mirror]?.error && (
                                  <span className="error-message">
                                    ({mirrorStatuses[mirror]?.error})
                                  </span>
                                )}
                              </>
                            )}
                          </span>
                        )}
                      </li>
                    ))}
                  </ul>
                </li>
              </ul>
            )}
          </div>
          <div className="info-section">
            <h3 onClick={() => setShowLocalInfo(!showLocalInfo)}>
              Local Information {showLocalInfo ? <FaChevronUp /> : <FaChevronDown />}
            </h3>
            {showLocalInfo && (
              <ul className="detail-list">
                <li>
                  <span>Installed:</span>
                  <span className="status-icon">{app.installed ? <FaCheck className="installed" /> : <FaTimes className="not-installed" />}</span>
                </li>
                <li><span>Installed Version:</span> <span>{app.state?.our_version || "Not installed"}</span></li>
                <li>
                  <span>Verified:</span>
                  <span className="status-icon">{app.state?.verified ? <FaCheck className="verified" /> : <FaTimes className="not-verified" />}</span>
                </li>
                <li><span>License:</span> <span>{app.metadata?.properties?.license || "Not specified"}</span></li>
                <li>
                  <span>Capabilities Approved:</span>
                  <button onClick={handleApproveCaps} className={`toggle-button ${app.state?.caps_approved ? 'active' : ''}`}>
                    {app.state?.caps_approved ? <FaCheck /> : <FaShieldAlt />}
                    {app.state?.caps_approved ? "Approved" : "Approve Caps"}
                  </button>
                </li>
                <li>
                  <span>Mirroring:</span>
                  <button onClick={handleMirror} className={`toggle-button ${app.state?.mirroring ? 'active' : ''}`}>
                    <FaMagnet />
                    {app.state?.mirroring ? "Mirroring" : "Start Mirroring"}
                  </button>
                </li>
                <li>
                  <span>Auto-Update:</span>
                  <button onClick={handleAutoUpdate} className={`toggle-button ${app.state?.auto_update ? 'active' : ''}`}>
                    <FaCog />
                    {app.state?.auto_update ? "Auto-Update On" : "Enable Auto-Update"}
                  </button>
                </li>
                <li><span>Manifest Hash:</span> <span className="hash">{app.state?.manifest_hash || "N/A"}</span></li>
              </ul>
            )}
          </div>
        </div>
        <div className="app-actions">
          {app.installed ? (
            <>
              <button onClick={handleLaunch} className="primary"><FaRocket /> Launch</button>
              <button onClick={handleUpdate} className="secondary"><FaSync /> Update</button>
              <button onClick={handleUninstall} className="secondary"><FaTrash /> Uninstall</button>
            </>
          ) : (
            <button onClick={handleInstall} className="primary"><FaDownload /> Install</button>
          )}
          {app.metadata?.external_url && (
            <a href={app.metadata.external_url} target="_blank" rel="noopener noreferrer" className="external-link">
              <FaLink /> External Link
            </a>
          )}
        </div>
      </div>

      {app.metadata?.properties?.screenshots && (
        <div className="app-screenshots">
          <h3>Screenshots</h3>
          <div className="screenshot-container">
            {app.metadata.properties.screenshots.map((screenshot, index) => (
              <img key={index} src={screenshot} alt={`Screenshot ${index + 1}`} className="app-screenshot" />
            ))}
          </div>
        </div>
      )}
    </section>
  );
}