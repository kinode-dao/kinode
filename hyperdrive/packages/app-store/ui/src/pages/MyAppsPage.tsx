import React, { useState, useEffect } from "react";
import { FaFolder, FaFile, FaChevronLeft, FaSync, FaRocket, FaSpinner, FaCheck, FaTrash, FaExclamationTriangle, FaTimesCircle, FaChevronDown, FaChevronRight } from "react-icons/fa";
import { useNavigate } from "react-router-dom";
import useAppsStore from "../store";
import { ResetButton } from "../components";
import { DownloadItem, PackageManifestEntry, PackageState, Updates, DownloadError, UpdateInfo } from "../types/Apps";

// Core packages that cannot be uninstalled
const CORE_PACKAGES = [
    "app-store:sys",
    "chess:sys",
    "contacts:sys",
    "homepage:sys",
    "hns-indexer:sys",
    "settings:sys",
    "terminal:sys",
];

export default function MyAppsPage() {
    const navigate = useNavigate();
    const {
        listings,
        fetchListings,
        fetchDownloads,
        fetchDownloadsForApp,
        startMirroring,
        stopMirroring,
        installApp,
        removeDownload,
        fetchInstalled,
        installed,
        uninstallApp,
        fetchUpdates,
        clearUpdates,
        updates
    } = useAppsStore();

    const [currentPath, setCurrentPath] = useState<string[]>([]);
    const [items, setItems] = useState<DownloadItem[]>([]);
    const [expandedUpdates, setExpandedUpdates] = useState<Set<string>>(new Set());
    const [isInstalling, setIsInstalling] = useState(false);
    const [isUninstalling, setIsUninstalling] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [showCapApproval, setShowCapApproval] = useState(false);
    const [manifest, setManifest] = useState<PackageManifestEntry | null>(null);
    const [selectedItem, setSelectedItem] = useState<DownloadItem | null>(null);
    const [showUninstallConfirm, setShowUninstallConfirm] = useState(false);
    const [appToUninstall, setAppToUninstall] = useState<any>(null);
    const [showAdvanced, setShowAdvanced] = useState(false);

    useEffect(() => {
        fetchInstalled();
        fetchListings();
        fetchUpdates();
        loadItems();
    }, [currentPath, fetchListings]);

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

    const handleClearUpdates = async (packageId: string) => {
        await clearUpdates(packageId);
        fetchUpdates(); // Refresh updates after clearing
    };

    const toggleUpdateExpansion = (packageId: string) => {
        setExpandedUpdates(prev => {
            const newSet = new Set(prev);
            if (newSet.has(packageId)) {
                newSet.delete(packageId);
            } else {
                newSet.add(packageId);
            }
            return newSet;
        });
    };

    const formatError = (error: DownloadError): string => {
        if (typeof error === 'string') {
            return error;
        } else if ('HashMismatch' in error) {
            return `Hash mismatch (expected ${error.HashMismatch.desired.slice(0, 8)}, got ${error.HashMismatch.actual.slice(0, 8)})`;
        } else if ('Timeout' in error) {
            return 'Connection timed out';
        }
        return 'Unknown error';
    };

    const renderUpdates = () => {
        if (!updates || Object.keys(updates).length === 0) {
            return (<></>);
        }

        return (
            <div className="updates-section">
                <h2 className="section-title">Failed Auto Updates ({Object.keys(updates).length})</h2>
                <div className="updates-list">
                    {Object.entries(updates).map(([packageId, versionMap]) => {
                        const totalErrors = Object.values(versionMap).reduce((sum, info) =>
                            sum + (info.errors?.length || 0), 0);
                        const hasManifestChanges = Object.values(versionMap).some(info =>
                            info.pending_manifest_hash);

                        return (
                            <div key={packageId} className="update-item error">
                                <div className="update-header" onClick={() => toggleUpdateExpansion(packageId)}>
                                    <div className="update-title">
                                        {expandedUpdates.has(packageId) ? <FaChevronDown /> : <FaChevronRight />}
                                        <FaExclamationTriangle className="error-badge" />
                                        <span>{packageId}</span>
                                        <div className="update-summary">
                                            {totalErrors > 0 && (
                                                <span className="error-count">{totalErrors} error{totalErrors !== 1 ? 's' : ''}</span>
                                            )}
                                            {hasManifestChanges && (
                                                <span className="manifest-badge">Manifest changes pending</span>
                                            )}
                                        </div>
                                    </div>
                                    <div className="update-actions">
                                        <button
                                            className="action-button retry"
                                            onClick={(e) => {
                                                e.stopPropagation();
                                                navigate(`/download/${packageId}`);
                                            }}
                                            title="Retry download"
                                        >
                                            <FaSync />
                                            <span>Retry</span>
                                        </button>
                                        <button
                                            className="action-button clear"
                                            onClick={(e) => {
                                                e.stopPropagation();
                                                handleClearUpdates(packageId);
                                            }}
                                            title="Clear update info"
                                        >
                                            <FaTimesCircle />
                                        </button>
                                    </div>
                                </div>
                                {expandedUpdates.has(packageId) && Object.entries(versionMap).map(([versionHash, info]) => (
                                    <div key={versionHash} className="update-details">
                                        <div className="version-info">
                                            Version: {versionHash.slice(0, 8)}...
                                        </div>
                                        {info.pending_manifest_hash && (
                                            <div className="manifest-info">
                                                <FaExclamationTriangle />
                                                Pending manifest: {info.pending_manifest_hash.slice(0, 8)}...
                                            </div>
                                        )}
                                        {info.errors && info.errors.length > 0 && (
                                            <div className="error-list">
                                                {info.errors.map(([source, error], idx) => (
                                                    <div key={idx} className="error-item">
                                                        <FaExclamationTriangle className="error-icon" />
                                                        <span>{source}: {formatError(error)}</span>
                                                    </div>
                                                ))}
                                            </div>
                                        )}
                                    </div>
                                ))}
                            </div>
                        );
                    })}
                </div>
            </div>
        );
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
                if (showAdvanced) {
                    await loadItems();
                }
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
            if (showAdvanced) {
                await loadItems();
            }
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
                if (showAdvanced) {
                    await loadItems();
                }
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
            if (showAdvanced) {
                await loadItems();
            }
            setShowUninstallConfirm(false);
            setAppToUninstall(null);
        } catch (error) {
            console.error('Uninstallation failed:', error);
            setError(`Uninstallation failed: ${error instanceof Error ? error.message : String(error)}`);
        } finally {
            setIsUninstalling(false);
        }
    };

    return (
        <div className="my-apps-page">
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '2rem' }}>
                <h1>Manage Installed Apps</h1>
                <ResetButton />
            </div>
            {error && <div className="error-message">{error}</div>}
            {renderUpdates()}

            <table className="apps-table">
                <tbody>
                    {(() => {
                        const userspaceApps = Object.values(installed).filter(app => !CORE_PACKAGES.includes(`${app.package_id.package_name}:${app.package_id.publisher_node}`));
                        if (userspaceApps.length === 0) {
                            return (
                                <tr>
                                    <td colSpan={2} style={{ textAlign: 'center' }}>
                                        No apps installed yet!
                                    </td>
                                </tr>
                            );
                        }
                        return userspaceApps.map((app) => {
                            const packageId = `${app.package_id.package_name}:${app.package_id.publisher_node}`;
                            const listing = listings?.[packageId];

                            return (
                                <tr key={packageId}>
                                    <td>
                                        {listing ? (
                                            <a href={`/main:app-store:sys/app/${packageId}`}>
                                                {listing.metadata?.name || packageId}
                                            </a>
                                        ) : (
                                            packageId
                                        )}
                                    </td>
                                    <td>
                                        <button
                                            onClick={() => initiateUninstall(app)}
                                            disabled={isUninstalling}
                                        >
                                            {isUninstalling ? <FaSpinner className="fa-spin" /> : <FaTrash />}
                                            Uninstall
                                        </button>
                                    </td>
                                </tr>
                            );
                        });
                    })()}
                </tbody>
            </table>

            <div className="advanced-section">
                <button
                    className="advanced-toggle"
                    onClick={() => {
                        setShowAdvanced(!showAdvanced);
                        if (showAdvanced) {
                            loadItems();
                        }
                    }}
                >
                    {showAdvanced ? <FaChevronDown /> : <FaChevronRight />} Advanced
                </button>

                {showAdvanced && (
                    <>
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
                        <h3>Core Apps</h3>
                        <table className="apps-table">
                            <tbody>
                                {Object.values(installed).map((app) => {
                                    const packageId = `${app.package_id.package_name}:${app.package_id.publisher_node}`;
                                    const isCore = CORE_PACKAGES.includes(packageId);

                                    if (!isCore) return null;

                                    return (
                                        <tr key={packageId}>
                                            <td>{packageId}</td>
                                        </tr>
                                    );
                                })}
                            </tbody>
                        </table>
                    </>
                )}
            </div>


            {/* Uninstall Confirmation Modal */}
            {
                showUninstallConfirm && appToUninstall && (
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
                )
            }

            {
                showCapApproval && manifest && (
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
                )
            }
        </div >
    );
}