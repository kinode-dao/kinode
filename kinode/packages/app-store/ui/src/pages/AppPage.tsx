import React, { useEffect, useState, useCallback, useMemo } from "react";
import { useParams } from "react-router-dom";
import { FaDownload, FaCheck, FaTimes, FaPlay, FaSpinner, FaTrash, FaSync, FaChevronDown, FaChevronUp, FaRocket } from "react-icons/fa";
import useAppsStore from "../store";
import { AppListing, PackageState, ManifestResponse } from "../types/Apps";
import { compareVersions } from "../utils/compareVersions";
import { MirrorSelector, ManifestDisplay } from '../components';

export default function AppPage() {
  const { id } = useParams();
  const {
    fetchListing,
    fetchInstalledApp,
    fetchDownloadsForApp,
    uninstallApp,
    setAutoUpdate,
    getLaunchUrl,
    fetchHomepageApps,
    downloadApp,
    downloads,
    activeDownloads,
    fetchData,
    installApp,
    clearAllActiveDownloads
  } = useAppsStore();

  const [app, setApp] = useState<AppListing | null>(null);
  const [installedApp, setInstalledApp] = useState<PackageState | null>(null);
  const [currentVersion, setCurrentVersion] = useState<string | null>(null);
  const [latestVersion, setLatestVersion] = useState<string | null>(null);
  const [upToDate, setUpToDate] = useState(false);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [isUninstalling, setIsUninstalling] = useState(false);
  const [isTogglingAutoUpdate, setIsTogglingAutoUpdate] = useState(false);
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [selectedMirror, setSelectedMirror] = useState<string>("");
  const [selectedVersion, setSelectedVersion] = useState<string>("");
  const [isMirrorOnline, setIsMirrorOnline] = useState<boolean | null>(null);
  const [showCapApproval, setShowCapApproval] = useState(false);
  const [manifestResponse, setManifestResponse] = useState<ManifestResponse | null>(null);
  const [isPolling, setIsPolling] = useState(false);
  const [canLaunch, setCanLaunch] = useState(false);

  const appDownloads = useMemo(() => downloads[id || ""] || [], [downloads, id]);

  const sortedVersions = useMemo(() => {
    if (!app || !app.metadata?.properties?.code_hashes) return [];
    return app.metadata.properties.code_hashes
      .map(([version, hash]) => ({ version, hash }))
      .sort((a, b) => {
        const vA = a.version.split('.').map(Number);
        const vB = b.version.split('.').map(Number);
        for (let i = 0; i < Math.max(vA.length, vB.length); i++) {
          if (vA[i] > vB[i]) return -1;
          if (vA[i] < vB[i]) return 1;
        }
        return 0;
      });
  }, [app]);

  const isDownloaded = useMemo(() => {
    if (!app || !selectedVersion) return false;
    const versionData = sortedVersions.find(v => v.version === selectedVersion);
    if (!versionData) return false;
    return appDownloads.some(d => d.File && d.File.name === `${versionData.hash}.zip`);
  }, [app, selectedVersion, sortedVersions, appDownloads]);

  const isDownloading = useMemo(() => {
    if (!app || !selectedVersion) return false;
    const versionData = sortedVersions.find(v => v.version === selectedVersion);
    if (!versionData) return false;
    return Object.keys(activeDownloads).some(key => key.startsWith(`${app.package_id.package_name}:`));
  }, [app, selectedVersion, sortedVersions, activeDownloads]);

  const downloadProgress = useMemo(() => {
    if (!isDownloading || !app || !selectedVersion) return null;
    const versionData = sortedVersions.find(v => v.version === selectedVersion);
    if (!versionData) return null;
    const activeDownloadKey = Object.keys(activeDownloads).find(key =>
      key.startsWith(`${app.package_id.package_name}:`)
    );
    if (!activeDownloadKey) return null;
    const progress = activeDownloads[activeDownloadKey];
    return progress ? Math.round((progress.downloaded / progress.total) * 100) : 0;
  }, [isDownloading, app, selectedVersion, sortedVersions, activeDownloads]);

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

      if (appData) {
        await fetchHomepageApps();
        setCanLaunch(!!getLaunchUrl(`${appData.package_id.package_name}:${appData.package_id.publisher_node}`));
      }

      if (appData?.metadata?.properties?.code_hashes) {
        const versions = appData.metadata.properties.code_hashes;
        if (versions.length > 0) {
          const latestVer = versions.reduce((latest, current) =>
            compareVersions(current[0], latest[0]) > 0 ? current : latest
          )[0];
          setLatestVersion(latestVer);
          setSelectedVersion(latestVer);

          if (installedAppData) {
            const installedVersion = versions.find(([_, hash]) => hash === installedAppData.our_version_hash);
            if (installedVersion) {
              setCurrentVersion(installedVersion[0]);
            }
            if (installedVersion && installedVersion[0] === latestVer) {
              setUpToDate(true);
            } else {
              setUpToDate(false);
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

  const handleMirrorSelect = useCallback((mirror: string, status: boolean | null | 'http') => {
    setSelectedMirror(mirror);
    setIsMirrorOnline(status === 'http' ? true : status);
  }, []);

  const handleDownload = useCallback(async () => {
    if (!id || !selectedMirror || !app || !selectedVersion) return;
    const versionData = sortedVersions.find(v => v.version === selectedVersion);
    if (versionData) {
      await downloadApp(id, versionData.hash, selectedMirror);
      // wait 1 second
      await new Promise(resolve => setTimeout(resolve, 1000));
      // Fetch updated downloads data
      const downloads = await fetchDownloadsForApp(id);
      // Now check for manifest in updated downloads
      const download = downloads.find(d => d.File && d.File.name === `${versionData.hash}.zip`);

      if (download?.File?.manifest) {
        try {
          const manifest_response: ManifestResponse = {
            package_id: app.package_id,
            version_hash: versionData.hash,
            manifest: download.File.manifest
          };
          setManifestResponse(manifest_response);
          setShowCapApproval(true);
        } catch (error) {
          console.error('Failed to parse manifest:', error);
        }
      } else {
        console.error('Manifest not found for the selected version');
      }
    }
  }, [id, selectedMirror, app, selectedVersion, sortedVersions, downloadApp]);

  const handleInstall = useCallback(async () => {
    if (!id || !selectedMirror || !app || !selectedVersion) return;
    const versionData = sortedVersions.find(v => v.version === selectedVersion);
    if (versionData) {
      const downloads = await fetchDownloadsForApp(id);
      // Now check for manifest in updated downloads
      const download = downloads.find(d => d.File && d.File.name === `${versionData.hash}.zip`);

      if (download?.File?.manifest) {
        try {
          const manifest_response: ManifestResponse = {
            package_id: app.package_id,
            version_hash: versionData.hash,
            manifest: download.File.manifest
          };
          setManifestResponse(manifest_response);
          setShowCapApproval(true);
        } catch (error) {
          console.error('Failed to parse manifest:', error);
        }
      } else {
        console.error('Manifest not found for the selected version');
      }
    }
  }, [id, selectedMirror, app, selectedVersion, sortedVersions, downloadApp]);

  const confirmInstall = useCallback(() => {
    if (!id || !selectedVersion) return;
    const versionData = sortedVersions.find(v => v.version === selectedVersion);
    if (versionData) {
      installApp(id, versionData.hash).then(() => {
        setShowCapApproval(false);
        setManifestResponse(null);
        fetchData(id);
        fetchHomepageApps();
        setIsPolling(true);
        setTimeout(() => {
          getLaunchUrl(`${app?.package_id.package_name}:${app?.package_id.publisher_node}`);
          loadData();
        }, 3000);
      });
    }
  }, [id, selectedVersion, sortedVersions, installApp, fetchData, fetchHomepageApps, fetchInstalledApp]);

  const handleLaunch = useCallback(() => {
    if (app) {
      const launchUrl = getLaunchUrl(`${app.package_id.package_name}:${app.package_id.publisher_node}`);
      if (launchUrl) {
        window.location.href = window.location.origin.replace('//app-store-sys.', '//') + launchUrl;
      }
    }
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
    clearAllActiveDownloads();
  }, [loadData, fetchHomepageApps, clearAllActiveDownloads]);

  useEffect(() => {
    if (isPolling) {
      const pollInterval = setInterval(() => {
        fetchHomepageApps();
      }, 1000);

      setTimeout(() => {
        setIsPolling(false);
        clearInterval(pollInterval);
      }, 5000);

      return () => clearInterval(pollInterval);
    }
  }, [isPolling, fetchHomepageApps]);

  if (isLoading) {
    return (
      <div className="app-page" style={{ minHeight: '100vh' }}>
        <div style={{ height: '200px', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
          <h4>Loading app details...</h4>
        </div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="app-page" style={{ minHeight: '100vh' }}>
        <div style={{ height: '200px', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
          <h4>{error}</h4>
        </div>
      </div>
    );
  }

  if (!app) {
    return (
      <div className="app-page" style={{ minHeight: '100vh' }}>
        <div style={{ height: '200px', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
          <h4>App details not found for {id}</h4>
        </div>
      </div>
    );
  }

  const valid_wit_version = app.metadata?.properties?.wit_version === 1;
  const canDownload = selectedMirror && (isMirrorOnline === true || selectedMirror.startsWith('http')) && !isDownloading && !isDownloaded;

  return (
    <div className="app-page" style={{ minHeight: '100vh' }}>
      {showCapApproval && manifestResponse && (
        <div className="cap-approval-popup">
          <div className="cap-approval-content">
            <h3>Approve Capabilities</h3>
            <ManifestDisplay manifestResponse={manifestResponse} />
            <div className="approval-buttons">
              <button onClick={() => setShowCapApproval(false)}>Cancel</button>
              <button onClick={confirmInstall}>
                Approve and Install
              </button>
            </div>
          </div>
        </div>
      )}
      <div className="app-header" style={{ minHeight: '200px' }}>
        <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: '1rem' }}>
          <div style={{ width: '128px', height: '128px', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
            <img
              src={app.metadata?.image || '/bird-orange.svg'}
              alt={app.metadata?.name || app.package_id.package_name}
              className="app-icon"
              style={{ maxWidth: '100%', maxHeight: '100%' }}
            />
          </div>
          <div className="app-title">
            <h2>{app.metadata?.name || app.package_id.package_name}</h2>
          </div>
        </div>
        <ul className="detail-list" style={{ minHeight: '150px' }}>
          <li>
            <span>Installed:</span>
            <span className="status-icon">
              {installedApp ? <FaCheck className="installed" /> : <FaTimes className="not-installed" />}
            </span>
          </li>
          {installedApp && <li>
            <span>Auto Update:</span>
            <span className="status-icon">
              {app.auto_update ? <FaCheck className="installed" /> : <FaTimes className="not-installed" />}
            </span>
          </li>}
          {latestVersion && (
            <>
              <li><span>Version:</span> <span>{latestVersion}</span></li>
            </>
          )}
          {currentVersion && latestVersion && currentVersion !== latestVersion && (
            <li><span>Installed Version:</span> <span>{currentVersion}</span></li>
          )}
          {currentVersion && latestVersion && currentVersion === latestVersion && (
            <li><span>Up to date:</span> <span className="status-icon">
              {upToDate ? <FaCheck className="installed" /> : <FaTimes className="not-installed" />}
            </span></li>
          )}
          {installedApp?.pending_update_hash && (
            <li className="warning">
              <span>Failed Auto-Update:</span>
              <span>Update to version with hash {installedApp.pending_update_hash.slice(0, 8)}... failed</span>
            </li>
          )}
          <li><span>Publisher:</span> <span>{app.package_id.publisher_node}</span></li>
          {app.metadata?.properties?.license
            ? <li><span>License:</span> <span>{app.metadata?.properties?.license}</span></li>
            : <></>}
        </ul>
      </div>

      {valid_wit_version ? <></> : <div className="app-warning">This app must be updated to 1.0</div>}

      <div className="app-actions" style={{ minHeight: '50px' }}>
        {installedApp && (
          <>
            {canLaunch && (
              <button onClick={handleLaunch} className="primary">
                <FaPlay /> Launch
              </button>
            )}
            <button onClick={handleUninstall} className="secondary" disabled={isUninstalling}>
              {isUninstalling ? <FaSpinner className="fa-spin" /> : <FaTrash />} Uninstall
            </button>
            <button onClick={handleToggleAutoUpdate} className="secondary" disabled={isTogglingAutoUpdate}>
              {isTogglingAutoUpdate ? <FaSpinner className="fa-spin" /> : <FaSync />}
              {app.auto_update ? " Disable" : " Enable"} Auto Update
            </button>
          </>
        )}
        {valid_wit_version && !upToDate && (
          <div className="download-section">
            {isDownloaded ? <button
              onClick={handleInstall}
              className="primary"
            >
              <FaDownload /> Install
            </button> : (
              <button
                onClick={handleDownload}
                disabled={!canDownload}
                className="primary"
              >
                {isDownloading ? (
                  <>
                    <FaSpinner className="fa-spin" /> Downloading... {downloadProgress}%
                  </>
                ) : (
                  <>
                    <FaDownload /> Download
                  </>
                )}
              </button>)}
          </div>
        )}
      </div>

      {app.metadata?.properties?.screenshots && (
        <div className="app-screenshots">
          <h3>Screenshots</h3>
          <div className="screenshot-container" style={{ minHeight: '200px' }}>
            {app.metadata.properties.screenshots.map((screenshot, index) => (
              <div key={index} style={{ aspectRatio: '16/9', width: '100%', maxWidth: '600px' }}>
                <img
                  src={screenshot}
                  alt={`Screenshot ${index + 1}`}
                  className="app-screenshot"
                  style={{ width: '100%', height: '100%', objectFit: 'contain' }}
                  loading="lazy"
                />
              </div>
            ))}
          </div>
        </div>
      )}

      <div className="app-description" style={{ minHeight: '100px' }}>
        {app.metadata?.description || "No description available"}
      </div>

      {valid_wit_version && !upToDate && (
        <>
          <button onClick={() => setShowAdvanced(!showAdvanced)} className="secondary">
            {showAdvanced ? <FaChevronUp /> : <FaChevronDown />} Advanced Download Options
          </button>
          <div className="advanced-options" style={{
            display: showAdvanced ? 'block' : 'none',
            minHeight: showAdvanced ? '100px' : '0'
          }}>
            <select
              value={selectedVersion}
              onChange={(e) => setSelectedVersion(e.target.value)}
              className="version-selector"
            >
              <option value="">Select version</option>
              {sortedVersions.map((version) => (
                <option key={version.version} value={version.version}>
                  {version.version}
                </option>
              ))}
            </select>
            <MirrorSelector
              packageId={id}
              onMirrorSelect={handleMirrorSelect}
            />
          </div>
        </>
      )}
    </div>
  )
};
