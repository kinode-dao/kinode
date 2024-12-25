import React, { useEffect, useState, useCallback, useMemo } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { FaDownload, FaCheck, FaTimes, FaPlay, FaSpinner, FaTrash, FaSync } from "react-icons/fa";
import useAppsStore from "../store";
import { AppListing, PackageState } from "../types/Apps";
import { compareVersions } from "../utils/compareVersions";

export default function AppPage() {
  const { id } = useParams();
  const navigate = useNavigate();
  const { fetchListing, fetchInstalledApp, uninstallApp, setAutoUpdate, getLaunchUrl, fetchHomepageApps } = useAppsStore();
  const [app, setApp] = useState<AppListing | null>(null);
  const [installedApp, setInstalledApp] = useState<PackageState | null>(null);
  const [currentVersion, setCurrentVersion] = useState<string | null>(null);
  const [latestVersion, setLatestVersion] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [isUninstalling, setIsUninstalling] = useState(false);
  const [isTogglingAutoUpdate, setIsTogglingAutoUpdate] = useState(false);


  const loadData = useCallback(async () => {
    if (!id) return;
    setIsLoading(true);
    setError(null);

    try {
      const [appData, installedAppData] = await Promise.all([
        fetchListing(id),
        fetchInstalledApp(id)
      ]);

      setApp(appData);
      setInstalledApp(installedAppData);

      if (appData?.metadata?.properties?.code_hashes) {
        const versions = appData.metadata.properties.code_hashes;
        if (versions.length > 0) {
          const latestVer = versions.reduce((latest, current) =>
            compareVersions(current[0], latest[0]) > 0 ? current : latest
          )[0];
          setLatestVersion(latestVer);

          if (installedAppData) {
            const installedVersion = versions.find(([_, hash]) => hash === installedAppData.our_version_hash);
            if (installedVersion) {
              setCurrentVersion(installedVersion[0]);
            }
          }
        }
      }
    } catch (err) {
      setError("Failed to load app details. Please try again.");
      console.error(err);
    } finally {
      setIsLoading(false);
    }
  }, [id, fetchListing, fetchInstalledApp]);


  const handleLaunch = useCallback(() => {
    if (app) {
      const launchUrl = getLaunchUrl(`${app.package_id.package_name}:${app.package_id.publisher_node}`);
      if (launchUrl) {
        window.location.href = window.location.origin.replace('//app-store-sys.', '//') + launchUrl;
      }
    }
  }, [app, getLaunchUrl]);

  const canLaunch = useMemo(() => {
    if (!app) return false;
    return !!getLaunchUrl(`${app.package_id.package_name}:${app.package_id.publisher_node}`);
  }, [app, getLaunchUrl]);

  const handleUninstall = async () => {
    if (!app) return;
    setIsUninstalling(true);
    try {
      await uninstallApp(`${app.package_id.package_name}:${app.package_id.publisher_node}`);
      await loadData();
    } catch (error) {
      console.error('Uninstallation failed:', error);
      setError(`Uninstallation failed: ${error instanceof Error ? error.message : String(error)}`);
    } finally {
      setIsUninstalling(false);
    }
  };

  const handleToggleAutoUpdate = async () => {
    if (!app || !latestVersion) return;
    setIsTogglingAutoUpdate(true);
    try {
      const newAutoUpdateState = !app.auto_update;
      await setAutoUpdate(`${app.package_id.package_name}:${app.package_id.publisher_node}`, latestVersion, newAutoUpdateState);
      await loadData();
    } catch (error) {
      console.error('Failed to toggle auto-update:', error);
      setError(`Failed to toggle auto-update: ${error instanceof Error ? error.message : String(error)}`);
    } finally {
      setIsTogglingAutoUpdate(false);
    }
  };

  useEffect(() => {
    loadData();
    fetchHomepageApps();
  }, [loadData, fetchHomepageApps]);

  const handleDownload = () => {
    navigate(`/download/${id}`);
  };

  if (isLoading) {
    return <div className="app-page"><h4>Loading app details...</h4></div>;
  }

  if (error) {
    return <div className="app-page"><h4>{error}</h4></div>;
  }

  if (!app) {
    return <div className="app-page"><h4>App details not found for {id}</h4></div>;
  }

  const valid_wit_version = app.metadata?.properties?.wit_version === 1

  return (
    <section className="app-page">
      <div className="app-header">
        {app.metadata?.image && (
          <img src={app.metadata.image} alt={app.metadata?.name || app.package_id.package_name} className="app-icon" />
        )}
        <div className="app-title">
          <h2>{app.metadata?.name || app.package_id.package_name}</h2>
          <p className="app-id">{`${app.package_id.package_name}.${app.package_id.publisher_node}`}</p>
        </div>
      </div>

      <div className="app-warning">
        {valid_wit_version ? <></> : "THIS APP MUST BE UPDATED TO 1.0"}
      </div>

      <div className="app-description">{app.metadata?.description || "No description available"}</div>

      <div className="app-info">
        <ul className="detail-list">
          <li>
            <span>Installed:</span>
            <span className="status-icon">{installedApp ? <FaCheck className="installed" /> : <FaTimes className="not-installed" />}</span>
          </li>
          {currentVersion && (
            <li><span>Current Version:</span> <span>{currentVersion}</span></li>
          )}
          {latestVersion && (
            <li><span>Latest Version:</span> <span>{latestVersion}</span></li>
          )}
          {installedApp?.pending_update_hash && (
            <li className="warning">
              <span>Failed Auto-Update:</span>
              <span>Update to version with hash {installedApp.pending_update_hash.slice(0, 8)}... failed, approve newly requested capabilities and install it here:</span>
            </li>
          )}
          <li><span>Publisher:</span> <span>{app.package_id.publisher_node}</span></li>
          {app.metadata?.properties?.license ? <li><span>License:</span> <span>app.metadata?.properties?.license</span></li> : <></>}
          <li>
            <span>Auto Update:</span>
            <span className="status-icon">
              {app.auto_update ? <FaCheck className="installed" /> : <FaTimes className="not-installed" />}
            </span>
          </li>
        </ul>
      </div>

      <div className="app-actions">
        {installedApp && (
          <>
            <button
              onClick={handleLaunch}
              className="primary"
              disabled={!canLaunch}
            >
              <FaPlay /> {canLaunch ? 'Launch' : 'No UI found for app'}
            </button>
            <button onClick={handleUninstall} className="secondary" disabled={isUninstalling}>
              {isUninstalling ? <FaSpinner className="fa-spin" /> : <FaTrash />} Uninstall
            </button>
            <button onClick={handleToggleAutoUpdate} className="secondary" disabled={isTogglingAutoUpdate}>
              {isTogglingAutoUpdate ? <FaSpinner className="fa-spin" /> : <FaSync />}
              {app.auto_update ? " Disable" : " Enable"} Auto Update
            </button>
          </>
        )}
        <button onClick={handleDownload} className="primary">
          <FaDownload /> Download
        </button>
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