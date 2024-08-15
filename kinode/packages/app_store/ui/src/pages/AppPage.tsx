import React, { useEffect, useState, useCallback } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { FaDownload, FaCheck, FaTimes, FaPlay } from "react-icons/fa";
import useAppsStore from "../store";
import { AppListing, PackageState } from "../types/Apps";
import { compareVersions } from "../utils/compareVersions";

export default function AppPage() {
  const { id } = useParams();
  const navigate = useNavigate();
  const { fetchListing, fetchInstalledApp, installApp } = useAppsStore();
  const [app, setApp] = useState<AppListing | null>(null);
  const [installedApp, setInstalledApp] = useState<PackageState | null>(null);
  const [currentVersion, setCurrentVersion] = useState<string | null>(null);
  const [latestVersion, setLatestVersion] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

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

      if (appData.metadata?.properties?.code_hashes) {
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

  useEffect(() => {
    loadData();
  }, [loadData]);

  const handleDownload = () => {
    navigate(`/download/${id}`);
  };

  const handleLaunch = () => {
    // Implement launch functionality
    console.log("Launching app:", app?.package_id.package_name);
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
          <li><span>Publisher:</span> <span>{app.package_id.publisher_node}</span></li>
          <li><span>License:</span> <span>{app.metadata?.properties?.license || "Not specified"}</span></li>
        </ul>
      </div>

      <div className="app-actions">
        <button onClick={handleDownload} className="primary">
          <FaDownload /> Download
        </button>
        {installedApp && (
          <button onClick={handleLaunch} className="primary">
            <FaPlay /> Launch
          </button>
        )}
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