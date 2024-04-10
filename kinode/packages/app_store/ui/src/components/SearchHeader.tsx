import React from "react";
import { useLocation, useNavigate } from "react-router-dom";
import {
  FaArrowLeft,
  FaDownload,
  FaMagnifyingGlass,
  FaUpload,
  FaX,
} from "react-icons/fa6";

import { MY_APPS_PATH } from "../constants/path";
import classNames from "classnames";

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
    <div className="flex justify-between">
      {location.pathname !== '/' ? (
        <button className="flex flex-col c mr-1 icon" onClick={() => {
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
          className="flex flex-col c mr-1 alt"
          onClick={() => navigate("/publish")}
        >
          <FaUpload />
        </button>
      )}
      {!hideSearch && (
        <div className="flex mx-2 flex-1 rounded-md">
          <button
            className="icon"
            type="button"
            onClick={() => inputRef.current?.focus()}
          >
            <FaMagnifyingGlass />
          </button>
          <input
            type="text"
            ref={inputRef}
            onChange={(event) => onChange(event.target.value)}
            value={value}
            placeholder="Search for apps..."
            className="w-full ml-2"
          />
          {value.length > 0 && <button
            className="icon ml-2"
            onClick={() => onChange("")}
          >
            <FaX />
          </button>}
        </div>
      )}
      <div className="flex">
        <button
          className={classNames("flex ml-1 alt")}
          onClick={() => (isMyAppsPage ? navigate(-1) : navigate(MY_APPS_PATH))}
        >
          <FaDownload className="mr-1" />
          My Apps
        </button>
      </div>
    </div>
  );
}
