import React from "react";
import { useLocation, useNavigate } from "react-router-dom";
import {
  FaArrowLeft,
  FaDownload,
  FaRegTimesCircle,
  FaSearch,
  FaUpload,
} from "react-icons/fa";

import { MY_APPS_PATH } from "../constants/path";

interface SearchHeaderProps {
  value?: string;
  onChange?: (value: string) => void;
  onBack?: () => void;
  onlyMyApps?: boolean;
  hideSearch?: boolean;
}

export default function SearchHeader({
  value = "",
  onChange = () => null,
  onBack,
  hideSearch = false,
}: SearchHeaderProps) {
  const navigate = useNavigate();
  const location = useLocation();
  const inputRef = React.useRef<HTMLInputElement>(null);

  const canGoBack = location.key !== "default";
  const isMyAppsPage = location.pathname === MY_APPS_PATH;

  return (
    <div className="search-header row between">
      {location.pathname !== '/' ? (
        <button className="back-btn col center" onClick={() => {
          if (onBack) {
            onBack()
          } else {
            canGoBack ? navigate(-1) : navigate('/')
          }
        }}>
          <FaArrowLeft />
        </button>
      ) : (
        <button
          className="back-btn col center"
          onClick={() => navigate("/publish")}
        >
          <FaUpload />
        </button>
      )}
      {!hideSearch && (
        <div className="searchbar row">
          <FaSearch
            className="search-icon"
            onClick={() => inputRef.current?.focus()}
          />
          <input
            ref={inputRef}
            onChange={(event) => onChange(event.target.value)}
            value={value}
            placeholder="Search for apps..."
          />
          {value.length > 0 && (
            <FaRegTimesCircle
              className="search-icon"
              style={{ margin: "0 -0.25em 0 0.25em" }}
              onClick={() => onChange("")}
            />
          )}
        </div>
      )}
      <div className="row">
        <button
          className={`my-pkg-btn row ${isMyAppsPage ? "selected" : ""}`}
          onClick={() => (isMyAppsPage ? navigate(-1) : navigate(MY_APPS_PATH))}
        >
          <FaDownload style={{ marginRight: "0.5em" }} />
          My Packages
        </button>
      </div>
    </div>
  );
}
