import React from 'react';

// @ts-ignore
import birdWhite from '../assets/icons/bird-white.svg';
// @ts-ignore
import birdOrange from '../assets/icons/bird-orange.svg';
// @ts-ignore
import birdPlain from '../assets/icons/bird-plain.svg';
// @ts-ignore
import kOrange from '../assets/icons/k-orange.svg';
// @ts-ignore
import kPlain from '../assets/icons/k-plain.svg';
// @ts-ignore
import kWhite from '../assets/icons/k-white.svg';
// @ts-ignore
import kBirdOrange from '../assets/icons/kbird-orange.svg';
// @ts-ignore
import kBirdPlain from '../assets/icons/kbird-plain.svg';
// @ts-ignore
import kBirdWhite from '../assets/icons/kbird-white.svg';
// @ts-ignore
import kBranchOrange from '../assets/icons/kbranch-orange.svg';
// @ts-ignore
import kBranchPlain from '../assets/icons/kbranch-plain.svg';
// @ts-ignore
import kBranchWhite from '../assets/icons/kbranch-white.svg';
// @ts-ignore
import kFlowerOrange from '../assets/icons/kflower-orange.svg';
// @ts-ignore
import kFlowerPlain from '../assets/icons/kflower-plain.svg';
// @ts-ignore
import kFlowerWhite from '../assets/icons/kflower-white.svg';

import { isMobileCheck } from '../utils/dimensions';
import classNames from 'classnames';

const AppIconPlaceholder: React.FC<{ text: string, className?: string, size: 'small' | 'medium' | 'large' }> = ({ text, className, size }) => {
  const index = Number.parseInt(text.split('').pop() || '0', 16)
  if (!index || isNaN(index)) {
    return null
  }

  const images = [
    birdWhite,
    birdOrange,
    birdPlain,
    kOrange,
    kPlain,
    kWhite,
    kBirdOrange,
    kBirdPlain,
    kBirdWhite,
    kBranchOrange,
    kBranchPlain,
    kBranchWhite,
    kFlowerOrange,
    kFlowerPlain,
    kFlowerWhite,
  ]

  const derivedFilename = images[index % images.length]

  if (!derivedFilename) {
    return null
  }

  const isMobile = isMobileCheck()

  return <img
    src={derivedFilename}
    className={classNames('m-0 align-self-center rounded-full', {
      'h-32 w-32': !isMobile && size === 'large',
      'h-18 w-18': !isMobile && size === 'medium',
      'h-12 w-12': isMobile || size === 'small',
    }, className)}
  />
}

export default AppIconPlaceholder