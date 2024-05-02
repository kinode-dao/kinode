import React from "react";
import { useLocation, useNavigate } from "react-router-dom";
import {
  FaArrowLeft,
  FaDownload,
  FaMagnifyingGlass,
  FaUpload,
  FaX,
} from "react-icons/fa6";

import { MY_APPS_PATH, PUBLISH_PATH } from "../constants/path";
import classNames from "classnames";
import { FaHome } from "react-icons/fa";

interface SearchHeaderProps {
  value?: string;
  onChange?: (value: string) => void;
  onBack?: () => void;
  onlyMyApps?: boolean;
  hideSearch?: boolean;
  hidePublish?: boolean;
}

export default function SearchHeader({
  value = "",
  onChange = () => null,
  onBack,
  hideSearch = false,
  hidePublish = false,
}: SearchHeaderProps) {
  const navigate = useNavigate();
  const location = useLocation();
  const inputRef = React.useRef<HTMLInputElement>(null);

  const canGoBack = location.key !== "default";
  const isMyAppsPage = location.pathname === MY_APPS_PATH;

  return (
    <div className="flex justify-between">
      {location.pathname !== '/' ? (
        <button className="flex flex-col c mr-2 icon" onClick={() => {
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
          className="flex flex-col c mr-2 icon"
          onClick={() => window.location.href = '/'}
        >
          <FaHome />
        </button>
      )}
      {!hidePublish && <button
        className="flex flex-col c mr-2 icon"
        onClick={() => navigate(PUBLISH_PATH)}
      >
        <FaUpload />
      </button>}
      {!hideSearch && (
        <div className="flex mr-2 flex-1 rounded-md">
          <button
            className="icon mr-2"
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
            className="w-full mr-2"
          />
          {value.length > 0 && <button
            className="icon"
            onClick={() => onChange("")}
          >
            <FaX />
          </button>}
        </div>
      )}
      <div className="flex">
        <button
          className={classNames("flex alt")}
          onClick={() => (isMyAppsPage ? navigate(-1) : navigate(MY_APPS_PATH))}
        >
          <FaDownload className="mr-2" />
          <span>My Apps</span>
        </button>
      </div>
    </div>
  );
}
