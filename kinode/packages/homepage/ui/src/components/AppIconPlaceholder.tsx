import React from 'react';

import birdWhite from '../assets/icons/bird-white.svg';
import birdOrange from '../assets/icons/bird-orange.svg';
import birdPlain from '../assets/icons/bird-plain.svg';
import kOrange from '../assets/icons/k-orange.svg';
import kPlain from '../assets/icons/k-plain.svg';
import kWhite from '../assets/icons/k-white.svg';
import kBirdOrange from '../assets/icons/kbird-orange.svg';
import kBirdPlain from '../assets/icons/kbird-plain.svg';
import kBirdWhite from '../assets/icons/kbird-white.svg';
import kBranchOrange from '../assets/icons/kbranch-orange.svg';
import kBranchPlain from '../assets/icons/kbranch-plain.svg';
import kBranchWhite from '../assets/icons/kbranch-white.svg';
import kFlowerOrange from '../assets/icons/kflower-orange.svg';
import kFlowerPlain from '../assets/icons/kflower-plain.svg';
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