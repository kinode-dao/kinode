import React from "react";
import { useLocation, useNavigate } from "react-router-dom";
import {
  FaArrowLeft,
  FaDownload,
  FaMagnifyingGlass,
  FaUpload,
} from "react-icons/fa6";

import { MY_APPS_PATH, PUBLISH_PATH } from "../constants/path";
import classNames from "classnames";
import { isMobileCheck } from "../utils/dimensions";
import HomeButton from "./HomeButton";
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
  const isMobile = isMobileCheck()

  return (
    <div className={classNames("flex justify-between", {
      "gap-4": isMobile,
      "gap-8": !isMobile
    })}>
      {location.pathname !== '/'
        ? <button
          className="flex flex-col c icon icon-orange"
          onClick={() => {
            if (onBack) {
              onBack()
            } else {
              canGoBack ? navigate(-1) : navigate('/')
            }
          }}
        >
          <FaArrowLeft />
        </button>
        : isMobile
          ? <button
            className={classNames("icon icon-orange", {
            })}
            onClick={() => window.location.href = '/'}
          >
            <FaHome />
          </button>
          : <></>}
      {!hidePublish && <button
        className="flex flex-col c icon icon-orange"
        onClick={() => navigate(PUBLISH_PATH)}
      >
        <FaUpload />
      </button>}
      {!hideSearch && (
        <div className="flex flex-1 rounded-md relative">
          <input
            type="text"
            ref={inputRef}
            onChange={(event) => onChange(event.target.value)}
            value={value}
            placeholder="Search for apps..."
            className="w-full self-stretch grow"
          />
          <button
            className={classNames("icon border-0 absolute top-1/2 -translate-y-1/2", {
              'right-2': isMobile,
              'right-4': !isMobile
            })}
            type="button"
            onClick={() => inputRef.current?.focus()}
          >
            <FaMagnifyingGlass />
          </button>
        </div>
      )}
      <button
        className={classNames("flex c", {
          "gap-4": isMobile,
          "gap-8 basis-1/5": !isMobile
        })}
        onClick={() => (isMyAppsPage ? navigate(-1) : navigate(MY_APPS_PATH))}
      >
        {!isMobile && <span>My Apps</span>}
        <FaDownload />
      </button>
    </div>
  );
}
