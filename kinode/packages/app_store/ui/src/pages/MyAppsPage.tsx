import React, { useState, useEffect } from "react";
import { FaUpload } from "react-icons/fa";
import { useNavigate, Link } from "react-router-dom";

import { AppInfo } from "../types/Apps";
import useAppsStore from "../store";
import { PUBLISH_PATH } from "../constants/path";

export default function MyAppsPage() {
  const { apps, getApps } = useAppsStore();
  const navigate = useNavigate();

  const [searchQuery, setSearchQuery] = useState<string>("");

  useEffect(() => {
    getApps();
  }, [getApps]);

  const filteredApps = apps.filter((app) =>
    app.package.toLowerCase().includes(searchQuery.toLowerCase()) ||
    app.metadata?.description?.toLowerCase().includes(searchQuery.toLowerCase())
  );

  const categorizedApps = {
    installed: filteredApps.filter(app => app.installed),
    downloaded: filteredApps.filter(app => !app.installed && app.state),
    available: filteredApps.filter(app => !app.state)
  };

  return (
    <div className="my-apps-page">
      <div className="my-apps-header">
        <h1>My Packages</h1>
        <button className="publish-button" onClick={() => navigate(PUBLISH_PATH)}>
          <FaUpload className="mr-2" />
          Publish Package
        </button>
      </div>

      <input
        type="text"
        placeholder="Search packages..."
        value={searchQuery}
        onChange={(e) => setSearchQuery(e.target.value)}
        className="search-input"
      />

      <div className="apps-list">
        {Object.entries(categorizedApps).map(([category, apps]) => (
          apps.length > 0 && (
            <div key={category} className="app-category">
              <h2>{category.charAt(0).toUpperCase() + category.slice(1)}</h2>
              {apps.map((app) => (
                <AppEntry key={app.package} app={app} />
              ))}
            </div>
          )
        ))}
      </div>
    </div>
  );
}

interface AppEntryProps {
  app: AppInfo;
}

const AppEntry: React.FC<AppEntryProps> = ({ app }) => {
  return (
    <Link to={`/apps/${app.package}`} className="app-entry">
      <h3>{app.metadata?.name || app.package}</h3>
      <p>{app.metadata?.description || "No description available"}</p>
    </Link>
  );
};