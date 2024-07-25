import React from 'react';
import { FaEllipsisH } from 'react-icons/fa';
import { Menu, MenuButton } from '@szhsin/react-menu';

interface DropdownProps extends React.HTMLAttributes<HTMLDivElement> {
}

export default function Dropdown({ ...props }: DropdownProps) {
  return (
    <Menu
      {...props}
      unmountOnClose={true}
      direction='left'
      menuButton={<MenuButton>
        <FaEllipsisH className='mb-[3px]' />
      </MenuButton>}
    >
      {props.children}
    </Menu>
  )
}
