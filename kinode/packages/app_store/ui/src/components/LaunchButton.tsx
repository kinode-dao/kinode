import React, { useCallback, useEffect, useState } from "react";
import { AppInfo } from "../types/Apps";
import classNames from "classnames";
import { FaPlay } from "react-icons/fa6";

interface LaunchButtonProps extends React.HTMLAttributes<HTMLButtonElement> {
  app: AppInfo;
  launchPath: string;
  isIcon?: boolean;
}

export default function LaunchButton({ app, launchPath, isIcon = false, ...props }: LaunchButtonProps) {
  const onLaunch = useCallback(async (e: React.MouseEvent<HTMLButtonElement>) => {
    e.preventDefault();
    window.location.href = `/${launchPath.replace('/', '')}`
    return;
  }, [app, launchPath]);

  return (
    <>
      <button
        {...props}
        type="button"
        className={classNames("text-sm self-start", props.className, {
          'icon clear': isIcon,
          'alt': !isIcon
        })}
        onClick={onLaunch}
      >
        {isIcon ? <FaPlay /> : "Launch"}
      </button>
    </>
  );
}
