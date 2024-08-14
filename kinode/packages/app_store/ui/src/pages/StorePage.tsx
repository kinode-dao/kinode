import React, { useState, useEffect } from "react";
import useAppsStore from "../store";
import { AppListing } from "../types/Apps";
import { Link } from "react-router-dom";

export default function StorePage() {
  const { listings, fetchListings } = useAppsStore();
  const [searchQuery, setSearchQuery] = useState<string>("");

  useEffect(() => {
    fetchListings();
  }, [fetchListings]);

  const filteredApps = Array.isArray(listings)
    ? listings.filter((app) =>
      app.package_id.package_name.toLowerCase().includes(searchQuery.toLowerCase()) ||
      app.metadata?.description?.toLowerCase().includes(searchQuery.toLowerCase())
    )
    : [];

  return (
    <div className="store-page">
      <div className="store-header">
        <input
          type="text"
          placeholder="Search apps..."
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
        />
      </div>
      <div className="app-list">
        {!Array.isArray(listings) ? (
          <p>Loading...</p>
        ) : listings.length === 0 ? (
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
                <AppRow key={`${app.package_id.package_name}:${app.package_id.publisher_node}`} app={app} />
              ))}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}

// ... rest of the code remains the same
interface AppRowProps {
  app: AppListing;
}

const AppRow: React.FC<AppRowProps> = ({ app }) => {
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