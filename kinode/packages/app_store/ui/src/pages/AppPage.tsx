import React, { useState, useEffect } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { FaDownload, FaSync, FaTrash, FaMagnet, FaCog, FaCheck, FaTimes, FaRocket, FaLink, FaChevronDown, FaChevronUp, FaShieldAlt, FaSpinner, FaPlay, FaExclamationTriangle } from "react-icons/fa";
import useAppsStore from "../store";
import { appId } from "../utils/app";
import { MirrorCheckFile } from "../types/Apps";

export default function AppPage() {
  const { installApp, updateApp, uninstallApp, approveCaps, setMirroring, setAutoUpdate, checkMirror, apps, downloadApp, getCaps, getApp } = useAppsStore();
  const { id } = useParams();

  const app = apps.find(a => appId(a) === id);
  const [showMetadata, setShowMetadata] = useState(true);
  const [showLocalInfo, setShowLocalInfo] = useState(true);
  const [mirrorStatuses, setMirrorStatuses] = useState<{ [mirror: string]: MirrorCheckFile | null }>({});
  const [selectedMirror, setSelectedMirror] = useState<string | null>(null);
  const [isDownloading, setIsDownloading] = useState(false);
  const [isInstalling, setIsInstalling] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [caps, setCaps] = useState<any>(null);
  const [showCaps, setShowCaps] = useState(false);
  const [localProgress, setLocalProgress] = useState<number | null>(null);

  // split these out a bit, this page is getting slightly too heavy
  const [latestVersion, setLatestVersion] = useState<string | null>(null);
  const [updateAvailable, setUpdateAvailable] = useState(false);

  const [showMetadataUri, setShowMetadataUri] = useState(false);
  const [metadataUriContent, setMetadataUriContent] = useState<any>(null);
  const [showAllVersions, setShowAllVersions] = useState(false);

  const [isUpdating, setIsUpdating] = useState(false);
  const [updateProgress, setUpdateProgress] = useState<number | null>(null);


  useEffect(() => {
    if (app) {
      checkMirrors();
      fetchCaps();
    }
  }, [app]);

  useEffect(() => {
    if (app && app.metadata_uri) {
      fetch(app.metadata_uri)
        .then(response => response.json())
        .then(data => setMetadataUriContent(data))
        .catch(error => console.error('Error fetching metadata URI:', error));
    }
  }, [app]);

  useEffect(() => {
    console.log("Checking app version updates...");
    if (app && app.metadata?.properties?.code_hashes) {
      const versions = Object.entries(app.metadata.properties.code_hashes);
      console.log("Available versions:", versions);
      if (versions.length > 0) {
        const [latestVersion, latestHash] = versions[versions.length - 1];
        console.log(`Latest version found: ${latestVersion} with hash ${latestHash}`);
        setLatestVersion(latestVersion);
        const isUpdateAvailable = app.state?.our_version !== latestHash;
        console.log(`Update available: ${isUpdateAvailable}`);
        setUpdateAvailable(isUpdateAvailable);
      }
    } else {
      console.log("No code hashes available in metadata.");
      // app metadata
      console.log("App metadata:", app);
      // app state
      console.log("App state:", app.state);
      // app publisher
      console.log("App publisher:", app.publisher);
    }
  }, [app]);
  if (!app) {
    return <div className="app-page"><h4>App details not found for {id}</h4></div>;
  }

  const checkMirrors = async () => {
    const mirrors = [app.publisher, ...(app.metadata?.properties?.mirrors || [])];
    const statuses: { [mirror: string]: MirrorCheckFile | null } = {};
    for (const mirror of mirrors) {
      const status = await checkMirror(mirror);
      statuses[mirror] = status;
    }
    setMirrorStatuses(statuses);
    setSelectedMirror(statuses[app.publisher]?.is_online ? app.publisher : mirrors.find(m => statuses[m]?.is_online) || null);
  };

  const fetchCaps = async () => {
    try {
      const appCaps = await getCaps(app);
      setCaps(appCaps);
    } catch (error) {
      console.error('Failed to fetch capabilities:', error);
      setError(`Failed to fetch capabilities: ${error instanceof Error ? error.message : String(error)}`);
    }
  };

  const handleDownload = async () => {
    if (selectedMirror) {
      setError(null);
      setIsDownloading(true);
      setLocalProgress(0);
      try {
        await downloadApp(app, selectedMirror);
        setLocalProgress(100);
        setTimeout(() => {
          setIsDownloading(false);
          setLocalProgress(null);
        }, 3000);
      } catch (error) {
        console.error('Download failed:', error);
        setError(`Download failed: ${error instanceof Error ? error.message : String(error)}`);
        setIsDownloading(false);
        setLocalProgress(null);
      }
    }
  };

  const handleUpdate = async () => {
    if (selectedMirror) {
      setError(null);
      setIsUpdating(true);
      setUpdateProgress(0);
      try {
        await updateApp(app, selectedMirror);
        setUpdateProgress(100);
        setTimeout(() => {
          setIsUpdating(false);
          setUpdateProgress(null);
          getApp(app.package); // Refresh app data after update
        }, 3000);
      } catch (error) {
        console.error('Update failed:', error);
        setError(`Update failed: ${error instanceof Error ? error.message : String(error)}`);
        setIsUpdating(false);
        setUpdateProgress(null);
      }
    }
  };

  const handleInstall = async () => {
    setIsInstalling(true);
    setError(null);
    try {
      if (!caps?.approved) {
        await approveCaps(app);
      }
      await installApp(app);
      await getApp(app.package);
    } catch (error) {
      console.error('Installation failed:', error);
      setError(`Installation failed: ${error instanceof Error ? error.message : String(error)}`);
    } finally {
      setIsInstalling(false);
    }
  };

  const handleUninstall = () => uninstallApp(app);
  const handleMirror = () => setMirroring(app, !app.state?.mirroring);
  const handleAutoUpdate = () => setAutoUpdate(app, !app.state?.auto_update);
  const handleLaunch = () => {
    console.log("Launching app:", app.package);
    window.open(`/${app.package}:${app.package}:${app.publisher}`, '_blank');
  };

  const isDownloaded = app.state !== null;
  const isInstalled = app.installed;

  const progressPercentage = updateProgress !== null
    ? updateProgress
    : localProgress !== null
      ? localProgress
      : isDownloaded ? 100 : 0;

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

      <div className="app-content">
        <div className="app-info-column">
          <div className="info-section">
            <h3 onClick={() => setShowMetadata(!showMetadata)}>
              Metadata {showMetadata ? <FaChevronUp /> : <FaChevronDown />}
            </h3>
            {showMetadata && (
              <ul className="detail-list">
                <li>
                  <span>Current Version:</span> <span>{app.state?.our_version || "Not installed"}</span>
                </li>
                <li className="expandable-item">
                  <div className="item-header">
                    <span>Latest Version:</span>
                    <span>{latestVersion || "Unknown"}</span>
                    <button
                      onClick={() => setShowAllVersions(!showAllVersions)}
                      className="dropdown-toggle"
                    >
                      {showAllVersions ? <FaChevronUp /> : <FaChevronDown />}
                    </button>
                  </div>
                  {showAllVersions && app.metadata?.properties?.code_hashes && (
                    <div className="json-container">
                      <pre className="json-display">
                        {JSON.stringify(app.metadata.properties.code_hashes, null, 2)}
                      </pre>
                    </div>
                  )}
                </li>
                {/* <li><span>Update Available:</span> <span>{updateAvailable ? "Yes" : "No"}</span></li> */}
                <li className="expandable-item">
                  <div className="item-header">
                    <span>~metadata-uri</span>
                    <span className="hash">{app.metadata_uri}</span>
                    <button
                      onClick={() => setShowMetadataUri(!showMetadataUri)}
                      className="dropdown-toggle"
                    >
                      {showMetadataUri ? <FaChevronUp /> : <FaChevronDown />}
                    </button>
                  </div>
                  {showMetadataUri && metadataUriContent && (
                    <div className="json-container">
                      <pre className="json-display">
                        {JSON.stringify(metadataUriContent, null, 2)}
                      </pre>
                    </div>
                  )}
                </li>
                <li><span>~metadata-hash</span> <span className="hash">{app.metadata_hash}</span></li>

                <li className="mirrors-list">
                  <span>Mirrors:</span>
                  <ul>
                    {Object.entries(mirrorStatuses).map(([mirror, status]) => (
                      <li key={mirror} className="mirror-item">
                        <span className="mirror-address">{mirror}</span>
                        <button
                          onClick={() => checkMirror(mirror)}
                          className="check-button"
                          title="Check if mirror is online"
                        >
                          <FaSync className={status === null ? 'spinning' : ''} />
                        </button>
                        {status && (
                          <span className="mirror-status">
                            {status.is_online ? (
                              <><FaCheck className="online" /> <span className="online">Online</span></>
                            ) : (
                              <>
                                <FaTimes className="offline" />
                                <span className="offline">Offline</span>
                                {status.error && (
                                  <span className="error-message">
                                    ({status.error})
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
                  <span className="status-icon">{isInstalled ? <FaCheck className="installed" /> : <FaTimes className="not-installed" />}</span>
                </li>
                <li><span>Installed Version:</span> <span>{app.state?.our_version || "Not installed"}</span></li>
                <li>
                  <span>Verified:</span>
                  <span className="status-icon">{app.state?.verified ? <FaCheck className="verified" /> : <FaTimes className="not-verified" />}</span>
                </li>
                <li><span>License:</span> <span>{app.metadata?.properties?.license || "Not specified"}</span></li>
                <li>
                  <span>Capabilities Approved:</span>
                  <button onClick={() => approveCaps(app)} className={`toggle-button ${app.state?.caps_approved ? 'active' : ''}`}>
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

        <div className="app-actions-column">
          <div className="app-actions">
            {isInstalled ? (
              <>
                <button onClick={handleLaunch} className="primary"><FaPlay /> Launch</button>
                {updateAvailable && (
                  <>
                    <div className="mirror-selection">
                      <select
                        value={selectedMirror || ''}
                        onChange={(e) => setSelectedMirror(e.target.value)}
                        disabled={isUpdating}
                      >
                        <option value="" disabled>Select Mirror</option>
                        {Object.entries(mirrorStatuses).map(([mirror, status]) => (
                          <option key={mirror} value={mirror} disabled={!status?.is_online}>
                            {mirror} {status?.is_online ? '(Online)' : '(Offline)'}
                          </option>
                        ))}
                      </select>
                    </div>
                    <button
                      onClick={handleUpdate}
                      className={`secondary ${isUpdating ? 'updating' : ''}`}
                      disabled={!selectedMirror || isUpdating}
                    >
                      {isUpdating ? (
                        <>
                          <FaSpinner className="fa-spin" /> Updating...
                        </>
                      ) : (
                        <>
                          <FaSync /> Update
                        </>
                      )}
                    </button>
                  </>
                )}
                <button onClick={handleUninstall} className="secondary"><FaTrash /> Uninstall</button>
              </>
            ) : (
              <>
                <div className="mirror-selection">
                  <select
                    value={selectedMirror || ''}
                    onChange={(e) => setSelectedMirror(e.target.value)}
                    disabled={isDownloading || isUpdating}
                  >
                    <option value="" disabled>Select Mirror</option>
                    {Object.entries(mirrorStatuses).map(([mirror, status]) => (
                      <option key={mirror} value={mirror} disabled={!status?.is_online}>
                        {mirror} {status?.is_online ? '(Online)' : '(Offline)'}
                      </option>
                    ))}
                  </select>
                </div>
                <button
                  className="download-button"
                  onClick={handleDownload}
                  disabled={!selectedMirror || isDownloading}
                >
                  {isDownloading ? (
                    <>
                      <FaSpinner className="fa-spin" /> Downloading...
                    </>
                  ) : (
                    <>
                      <FaDownload /> {isDownloaded ? 'Re-download' : 'Download'}
                    </>
                  )}
                </button>
                <button
                  className="install-button"
                  onClick={handleInstall}
                  disabled={!isDownloaded || isInstalling}
                >
                  <FaRocket /> {isInstalling ? 'Installing...' : 'Install'}
                </button>
              </>
            )}
            {app.metadata?.external_url && (
              <a href={app.metadata.external_url} target="_blank" rel="noopener noreferrer" className="external-link">
                <FaLink /> External Link
              </a>
            )}
          </div>


          {(isDownloading || isUpdating || isDownloaded) && (
            <div className="progress-container">
              <div className="progress-bar">
                <div
                  className="progress"
                  style={{ width: `${progressPercentage}%` }}
                ></div>
                <div className="progress-percentage">{progressPercentage}%</div>
              </div>
            </div>
          )}

          {error && (
            <div className="error-message">
              <FaExclamationTriangle /> {error}
            </div>
          )}

          <div className="capabilities-section">
            <h3 onClick={() => setShowCaps(!showCaps)}>
              Requested Capabilities {showCaps ? <FaChevronUp /> : <FaChevronDown />}
            </h3>
            {showCaps && caps && (
              <pre className="capabilities">{JSON.stringify(caps, null, 2)}</pre>
            )}
          </div>
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