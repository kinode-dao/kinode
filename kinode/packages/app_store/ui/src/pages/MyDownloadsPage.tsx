import React, { useState, useEffect } from "react";
import { FaFolder, FaFile, FaChevronLeft } from "react-icons/fa";
import useAppsStore from "../store";
import { DownloadItem } from "../types/Apps";

export default function MyDownloadsPage() {
    const { fetchDownloads, fetchDownloadsForApp, listings } = useAppsStore();
    const [currentPath, setCurrentPath] = useState<string[]>([]);
    const [items, setItems] = useState<DownloadItem[]>([]);

    useEffect(() => {
        loadItems();
    }, [currentPath]);

    const loadItems = async () => {
        let downloads: DownloadItem[];
        if (currentPath.length === 0) {
            downloads = await fetchDownloads();
        } else {
            downloads = await fetchDownloadsForApp(currentPath.join(':'));
        }
        setItems(downloads);
    };

    const navigateToItem = (item: DownloadItem) => {
        if (!item.is_file) {
            setCurrentPath([...currentPath, item.name]);
        }
    };

    const navigateUp = () => {
        setCurrentPath(currentPath.slice(0, -1));
    };

    const getItemVersion = (name: string): string | undefined => {
        if (currentPath.length === 0) {
            return undefined; // No version for top-level directories
        }

        const appId = currentPath.join(':');
        const app = listings.find(a => `${a.package_id.package_name}:${a.package_id.publisher_node}` === appId);

        if (app && app.metadata?.properties?.code_hashes) {
            const matchingHash = app.metadata.properties.code_hashes.find(([_, hash]) => `${hash}.zip` === name);
            return matchingHash ? matchingHash[0] : undefined;
        }

        return undefined;
    };

    return (
        <div className="downloads-page">
            <h2>Downloads</h2>
            <div className="file-explorer">
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
                            <th>Version</th>
                        </tr>
                    </thead>
                    <tbody>
                        {items.map((item, index) => {
                            const version = getItemVersion(item.name);
                            return (
                                <tr key={index} onClick={() => navigateToItem(item)} className={item.is_file ? 'file' : 'directory'}>
                                    <td>
                                        {item.is_file ? <FaFile /> : <FaFolder />} {item.name}
                                    </td>
                                    <td>{item.is_file ? 'File' : 'Directory'}</td>
                                    <td>{item.size ? `${(item.size / 1024).toFixed(2)} KB` : '-'}</td>
                                    <td>{version || '-'}</td>
                                </tr>
                            );
                        })}
                    </tbody>
                </table>
            </div>
        </div>
    );
}