import React, { useState, useEffect } from "react";
import useAppsStore from "../store";
import { AppInfo } from "../types/Apps";
import { appId } from '../utils/app'
import { Link } from "react-router-dom";
import { FaGlobe, FaPeopleGroup, FaCode } from "react-icons/fa6";

export default function StorePage() {
  const { apps, getApps, rebuildIndex } = useAppsStore();
  const [searchQuery, setSearchQuery] = useState<string>("");
  const [isRebuildingIndex, setIsRebuildingIndex] = useState(false);

  useEffect(() => {
    getApps();
  }, [getApps]);

  const filteredApps = apps.filter((app) =>
    app.package.toLowerCase().includes(searchQuery.toLowerCase()) ||
    app.metadata?.description?.toLowerCase().includes(searchQuery.toLowerCase())
  );

  const handleRebuildIndex = async () => {
    if (window.confirm('Are you sure you want to rebuild the app index? This may take a few seconds.')) {
      setIsRebuildingIndex(true);
      try {
        await rebuildIndex();
        await getApps();
      } catch (error) {
        console.error(error);
      } finally {
        setIsRebuildingIndex(false);
      }
    }
  };

  return (
    <div className="store-page">
      <div className="store-header">
        <div className="search-bar">
          <input
            type="text"
            placeholder="Search apps..."
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
          />
        </div>
        <button onClick={handleRebuildIndex} disabled={isRebuildingIndex}>
          {isRebuildingIndex ? "Rebuilding..." : "Rebuild Index"}
        </button>
      </div>
      <div className="app-list">
        <table>
          <thead>
            <tr>
              <th>Name</th>
              <th>Description</th>
              <th>Publisher</th>
              <th>Version</th>
              <th>Mirrors</th>
              <th>Status</th>
            </tr>
          </thead>
          <tbody>
            {filteredApps.map((app) => (
              <AppRow key={appId(app)} app={app} />
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

interface AppRowProps {
  app: AppInfo;
}

const AppRow: React.FC<AppRowProps> = ({ app }) => {
  const version = app.metadata?.properties?.current_version || "Unknown";
  const mirrors = app.metadata?.properties?.mirrors?.length || 0;

  return (
    <tr className="app-row">
      <td>
        <Link to={`/app/${appId(app)}`} className="app-name">
          {app.metadata?.name || app.package}
        </Link>
      </td>
      <td>{app.metadata?.description || "No description available"}</td>
      <td>
        <span className="publisher">
          <FaPeopleGroup /> {app.publisher}
        </span>
      </td>
      <td>
        <span className="version">
          <FaCode /> {version}
        </span>
      </td>
      <td>
        <span className="mirrors">
          <FaGlobe /> {mirrors}
        </span>
      </td>
      <td>
        <span className={`status ${app.installed ? 'installed' : 'not-installed'}`}>
          {app.installed ? 'Installed' : 'Not Installed'}
        </span>
      </td>
    </tr>
  );
};