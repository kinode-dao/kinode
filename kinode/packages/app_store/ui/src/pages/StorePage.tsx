import React, { useState, useEffect } from "react";
import useAppsStore from "../store";
import { AppListing } from "../types/Apps";
import { Link } from "react-router-dom";
import { FaSearch } from "react-icons/fa";

export default function StorePage() {
  const { listings, fetchListings } = useAppsStore();
  const [searchQuery, setSearchQuery] = useState<string>("");

  useEffect(() => {
    fetchListings();
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
      </div>
      <div className="app-list">
        {!listings ? (
          <p>Loading...</p>
        ) : filteredApps.length === 0 ? (
          <p>No apps available.</p>
        ) : (
          <table>
            <thead>
              <tr>
                <th></th>
                <th>Name</th>
                <th>Description</th>
                <th>Publisher</th>
              </tr>
            </thead>
            <tbody>
              {filteredApps.map((app) => (
                <AppRow key={`${app.package_id?.package_name}:${app.package_id?.publisher_node}`} app={app} />
              ))}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}

const AppRow: React.FC<{ app: AppListing }> = ({ app }) => {
  if (!app || !app.package_id) return null;

  return (
    <tr className="app-row">
      <td>
        {app.metadata?.image && (
          <img
            src={app.metadata.image}
            alt={`${app.metadata?.name || app.package_id.package_name} icon`}
            className="app-icon"
            width="32"
            height="32"
          />
        )}
      </td>
      <td>
        <Link to={`/app/${app.package_id.package_name}:${app.package_id.publisher_node}`} className="app-name">
          {app.metadata?.name || app.package_id.package_name}
        </Link>
      </td>
      <td>{app.metadata?.description || "No description available"}</td>
      <td>{app.package_id.publisher_node}</td>
    </tr>
  );
};