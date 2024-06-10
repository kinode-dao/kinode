import React from "react";
import { AppInfo } from "../types/Apps";
import { appId } from "../utils/app";
import classNames from "classnames";
import ColorDot from "./ColorDot";
import { isMobileCheck } from "../utils/dimensions";
import AppIconPlaceholder from './AppIconPlaceholder'

interface AppHeaderProps extends React.HTMLAttributes<HTMLDivElement> {
  app: AppInfo;
  size?: "small" | "medium" | "large";
  overrideImageSize?: "small" | "medium" | "large"
}

export default function AppHeader({
  app,
  size = "medium",
  overrideImageSize,
  ...props
}: AppHeaderProps) {
  const isMobile = isMobileCheck()

  const appName = <div
    className={classNames({
      'text-3xl font-[OpenSans]': !isMobile && size === 'large',
      'text-xl': !isMobile && size !== 'large',
      'text-lg': isMobile
    })}
  >
    {app.metadata?.name || appId(app)}
  </div>

  const imageSize = overrideImageSize || size

  return <div
    {...props}
    className={classNames('flex w-full justify-content-start', size, props.className, {
      'flex-col': size === 'small',
      'gap-2': isMobile,
      'gap-4': !isMobile,
      'gap-6': !isMobile && size === 'large'
    })}
  >
    {size === 'small' && appName}
    {app.metadata?.image
      ? <img
        src={app.metadata.image}
        alt="app icon"
        className={classNames('object-cover', {
          'rounded': !imageSize,
          'rounded-md': imageSize === 'small',
          'rounded-lg': imageSize === 'medium',
          'rounded-2xl': imageSize === 'large',
          'h-32': imageSize === 'large' || imageSize === 'small',
          'h-20': imageSize === 'medium',
        })}
      />
      : <AppIconPlaceholder
        text={app.metadata_hash || app.state?.our_version?.toString() || ''}
        size={imageSize}
      />}
    <div className={classNames("flex flex-col", {
      'gap-2': isMobile,
      'gap-4 max-w-3/4': isMobile && size !== 'small'
    })}>
      {size !== 'small' && appName}
      {app.metadata?.description && (
        <div
          style={{
            display: '-webkit-box',
            WebkitLineClamp: 2,
            WebkitBoxOrient: 'vertical',
            overflow: 'hidden',
            textOverflow: 'ellipsis'
          }}
          className={classNames({
            'text-2xl': size === 'large'
          })}
        >
          {app.metadata.description}
        </div>
      )}
    </div>
  </div>
}
