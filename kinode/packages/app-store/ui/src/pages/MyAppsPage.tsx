import React, { useState, useEffect } from "react";
import { FaFolder, FaFile, FaChevronLeft, FaSync, FaRocket, FaSpinner, FaCheck, FaTrash } from "react-icons/fa";
import useAppsStore from "../store";
import { DownloadItem, PackageManifest, PackageState } from "../types/Apps";

// Core packages that cannot be uninstalled
const CORE_PACKAGES = [
    "app_store:sys",
    "contacts:sys",
    "kino_updates:sys",
    "terminal:sys",
    "chess:sys",
    "kns_indexer:sys",
    "settings:sys",
    "homepage:sys"
];

export default function MyAppsPage() {
    const {
        fetchDownloads,
        fetchDownloadsForApp,
        startMirroring,
        stopMirroring,
        installApp,
        removeDownload,
        fetchInstalled,
        installed,
        uninstallApp
    } = useAppsStore();

    const [currentPath, setCurrentPath] = useState<string[]>([]);
    const [items, setItems] = useState<DownloadItem[]>([]);
    const [isInstalling, setIsInstalling] = useState(false);
    const [isUninstalling, setIsUninstalling] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [showCapApproval, setShowCapApproval] = useState(false);
    const [manifest, setManifest] = useState<PackageManifest | null>(null);
    const [selectedItem, setSelectedItem] = useState<DownloadItem | null>(null);
    const [showUninstallConfirm, setShowUninstallConfirm] = useState(false);
    const [appToUninstall, setAppToUninstall] = useState<any>(null);

    useEffect(() => {
        loadItems();
        fetchInstalled();
    }, [currentPath]);

    const loadItems = async () => {
        try {
            let downloads: DownloadItem[];
            if (currentPath.length === 0) {
                downloads = await fetchDownloads();
            } else {
                downloads = await fetchDownloadsForApp(currentPath.join(':'));
            }
            setItems(downloads);
        } catch (error) {
            console.error("Error loading items:", error);
            setError(`Error loading items: ${error instanceof Error ? error.message : String(error)}`);
        }
    };

    const initiateUninstall = (app: any) => {
        const packageId = `${app.package_id.package_name}:${app.package_id.publisher_node}`;
        if (CORE_PACKAGES.includes(packageId)) {
            setError("Cannot uninstall core system packages");
            return;
        }
        setAppToUninstall(app);
        setShowUninstallConfirm(true);
    };

    const handleUninstall = async () => {
        if (!appToUninstall) return;
        setIsUninstalling(true);
        const packageId = `${appToUninstall.package_id.package_name}:${appToUninstall.package_id.publisher_node}`;
        try {
            await uninstallApp(packageId);
            await fetchInstalled();
            await loadItems();
            setShowUninstallConfirm(false);
            setAppToUninstall(null);
        } catch (error) {
            console.error('Uninstallation failed:', error);
            setError(`Uninstallation failed: ${error instanceof Error ? error.message : String(error)}`);
        } finally {
            setIsUninstalling(false);
        }
    };


    const navigateToItem = (item: DownloadItem) => {
        if (item.Dir) {
            setCurrentPath([...currentPath, item.Dir.name]);
        }
    };

    const navigateUp = () => {
        setCurrentPath(currentPath.slice(0, -1));
    };

    const toggleMirroring = async (item: DownloadItem) => {
        if (item.Dir) {
            const packageId = [...currentPath, item.Dir.name].join(':');
            try {
                if (item.Dir.mirroring) {
                    await stopMirroring(packageId);
                } else {
                    await startMirroring(packageId);
                }
                await loadItems();
            } catch (error) {
                console.error("Error toggling mirroring:", error);
                setError(`Error toggling mirroring: ${error instanceof Error ? error.message : String(error)}`);
            }
        }
    };

    const handleInstall = async (item: DownloadItem) => {
        if (item.File) {
            setSelectedItem(item);
            try {
                const manifestData = JSON.parse(item.File.manifest);
                setManifest(manifestData);
                setShowCapApproval(true);
            } catch (error) {
                console.error('Failed to parse manifest:', error);
                setError(`Failed to parse manifest: ${error instanceof Error ? error.message : String(error)}`);
            }
        }
    };

    const confirmInstall = async () => {
        if (!selectedItem?.File) return;
        setIsInstalling(true);
        setError(null);
        try {
            const fileName = selectedItem.File.name;
            const parts = fileName.split(':');
            const versionHash = parts.pop()?.replace('.zip', '');

            if (!versionHash) throw new Error('Invalid file name format');

            const packageId = [...currentPath, ...parts].join(':');

            await installApp(packageId, versionHash);
            await fetchInstalled();
            setShowCapApproval(false);
            await loadItems();
        } catch (error) {
            console.error('Installation failed:', error);
            setError(`Installation failed: ${error instanceof Error ? error.message : String(error)}`);
        } finally {
            setIsInstalling(false);
        }
    };

    const handleRemoveDownload = async (item: DownloadItem) => {
        if (item.File) {
            try {
                const packageId = currentPath.join(':');
                const versionHash = item.File.name.replace('.zip', '');
                await removeDownload(packageId, versionHash);
                await loadItems();
            } catch (error) {
                console.error('Failed to remove download:', error);
                setError(`Failed to remove download: ${error instanceof Error ? error.message : String(error)}`);
            }
        }
    };

    const isAppInstalled = (name: string): boolean => {
        const packageName = name.replace('.zip', '');
        return Object.values(installed).some(app => app.package_id.package_name === packageName);
    };

    return (
        <div className="downloads-page">
            <h2>My Apps</h2>

            {/* Installed Apps Section */}
            <div className="file-explorer">
                <h3>Installed Apps</h3>
                <table className="downloads-table">
                    <thead>
                        <tr>
                            <th>Package ID</th>
                            <th>Actions</th>
                        </tr>
                    </thead>
                    <tbody>
                        {Object.values(installed).map((app) => {
                            const packageId = `${app.package_id.package_name}:${app.package_id.publisher_node}`;
                            const isCore = CORE_PACKAGES.includes(packageId);
                            return (
                                <tr key={packageId}>
                                    <td>{packageId}</td>
                                    <td>
                                        {isCore ? (
                                            <span className="core-package">Core Package</span>
                                        ) : (
                                            <button
                                                onClick={() => initiateUninstall(app)}
                                                disabled={isUninstalling}
                                            >
                                                {isUninstalling ? <FaSpinner className="fa-spin" /> : <FaTrash />}
                                                Uninstall
                                            </button>
                                        )}
                                    </td>
                                </tr>
                            );
                        })}
                    </tbody>
                </table>
            </div>

            {/* Downloads Section */}
            <div className="file-explorer">
                <h3>Downloads</h3>
                <div className="path-navigation">
                    {currentPath.length > 0 && (
                        <button onClick={navigateUp} className="navigate-up">
                            <FaChevronLeft /> Back
                        </button>
                    )}
                    <span className="current-path">/{currentPath.join('/')}</span>
                </div>
                <table className="downloads-table">
                    <thead>
                        <tr>
                            <th>Name</th>
                            <th>Type</th>
                            <th>Size</th>
                            <th>Mirroring</th>
                            <th>Actions</th>
                        </tr>
                    </thead>
                    <tbody>
                        {items.map((item, index) => {
                            const isFile = !!item.File;
                            const name = isFile ? item.File!.name : item.Dir!.name;
                            const isInstalled = isFile && isAppInstalled(name);
                            return (
                                <tr key={index} onClick={() => navigateToItem(item)} className={isFile ? 'file' : 'directory'}>
                                    <td>
                                        {isFile ? <FaFile /> : <FaFolder />} {name}
                                    </td>
                                    <td>{isFile ? 'File' : 'Directory'}</td>
                                    <td>{isFile ? `${(item.File!.size / 1024).toFixed(2)} KB` : '-'}</td>
                                    <td>{!isFile && (item.Dir!.mirroring ? 'Yes' : 'No')}</td>
                                    <td>
                                        {!isFile && (
                                            <button onClick={(e) => { e.stopPropagation(); toggleMirroring(item); }}>
                                                <FaSync /> {item.Dir!.mirroring ? 'Stop' : 'Start'} Mirroring
                                            </button>
                                        )}
                                        {isFile && !isInstalled && (
                                            <>
                                                <button onClick={(e) => { e.stopPropagation(); handleInstall(item); }}>
                                                    <FaRocket /> Install
                                                </button>
                                                <button onClick={(e) => { e.stopPropagation(); handleRemoveDownload(item); }}>
                                                    <FaTrash /> Delete
                                                </button>
                                            </>
                                        )}
                                        {isFile && isInstalled && (
                                            <FaCheck className="installed" />
                                        )}
                                    </td>
                                </tr>
                            );
                        })}
                    </tbody>
                </table>
            </div>

            {error && (
                <div className="error-message">
                    {error}
                </div>
            )}

            {/* Uninstall Confirmation Modal */}
            {showUninstallConfirm && appToUninstall && (
                <div className="cap-approval-popup">
                    <div className="cap-approval-content">
                        <h3>Confirm Uninstall</h3>
                        <div className="warning-message">
                            Are you sure you want to uninstall this app?
                        </div>
                        <div className="package-info">
                            <strong>Package ID:</strong> {`${appToUninstall.package_id.package_name}:${appToUninstall.package_id.publisher_node}`}
                        </div>
                        {appToUninstall.metadata?.name && (
                            <div className="package-info">
                                <strong>Name:</strong> {appToUninstall.metadata.name}
                            </div>
                        )}
                        <div className="approval-buttons">
                            <button
                                onClick={() => {
                                    setShowUninstallConfirm(false);
                                    setAppToUninstall(null);
                                }}
                            >
                                Cancel
                            </button>
                            <button
                                onClick={handleUninstall}
                                disabled={isUninstalling}
                                className="danger"
                            >
                                {isUninstalling ? <FaSpinner className="fa-spin" /> : 'Confirm Uninstall'}
                            </button>
                        </div>
                    </div>
                </div>
            )}



            {showCapApproval && manifest && (
                <div className="cap-approval-popup">
                    <div className="cap-approval-content">
                        <h3>Approve Capabilities</h3>
                        <pre className="json-display">
                            {JSON.stringify(manifest[0]?.request_capabilities || [], null, 2)}
                        </pre>
                        <div className="approval-buttons">
                            <button onClick={() => setShowCapApproval(false)}>Cancel</button>
                            <button onClick={confirmInstall} disabled={isInstalling}>
                                {isInstalling ? <FaSpinner className="fa-spin" /> : 'Approve and Install'}
                            </button>
                        </div>
                    </div>
                </div>
            )}
        </div>
    );
}