import React, { useState, useEffect } from "react";
import useAppsStore from "../store";
import { AppListing } from "../types/Apps";
import { Link } from "react-router-dom";
import { FaSearch } from "react-icons/fa";
import { ResetButton } from "../components";

export default function StorePage() {
  const { listings, fetchListings, fetchUpdates } = useAppsStore();
  const [searchQuery, setSearchQuery] = useState<string>("");

  useEffect(() => {
    fetchListings();
    fetchUpdates();
  }, [fetchListings]);

  // extensive temp null handling due to weird prod bug
  const filteredApps = React.useMemo(() => {
    if (!listings) return [];
    return Object.values(listings).filter((app) => {
      if (!app || !app.package_id) return false;
      const nameMatch = app.package_id.package_name.toLowerCase().includes(searchQuery.toLowerCase());
      const descMatch = app.metadata?.description?.toLowerCase().includes(searchQuery.toLowerCase()) || false;
      return nameMatch || descMatch;
    });
  }, [listings, searchQuery]);

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
          <FaSearch />
        </div>
        <ResetButton />
      </div>
      {!listings ? (
        <p>Loading...</p>
      ) : filteredApps.length === 0 ? (
        <p>No apps available.</p>
      ) : (
        <div className="app-grid">
          {filteredApps.map((app) => (
            <AppCard key={`${app.package_id?.package_name}:${app.package_id?.publisher_node}`} app={app} />
          ))}
        </div>
      )}
    </div>
  );
}

const AppCard: React.FC<{ app: AppListing }> = ({ app }) => {
  if (!app || !app.package_id) return null;

  return (
    <Link
      to={`/app/${app.package_id.package_name}:${app.package_id.publisher_node}`}
      className="app-card"
    >
      <div className="app-icon-wrapper">
        <img
          src={app.metadata?.image || '/bird-orange.svg'}
          alt={`${app.metadata?.name || app.package_id.package_name} icon`}
          className="app-icon"
        />
      </div>
      <h3 className="app-name">
        {app.metadata?.name || app.package_id.package_name}
      </h3>
      <p className="app-publisher">
        {app.package_id.publisher_node}
      </p>
      {app.metadata?.description && (
        <p className="app-description">
          {app.metadata.description.length > 100
            ? `${app.metadata.description.substring(0, 100)}...`
            : app.metadata.description}
        </p>
      )}
    </Link>
  );
};