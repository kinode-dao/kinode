import React from 'react';
import { FaEllipsisH } from 'react-icons/fa';
import { Menu, MenuButton } from '@szhsin/react-menu';

interface DropdownProps extends React.HTMLAttributes<HTMLDivElement> {
}

export default function Dropdown({ ...props }: DropdownProps) {
  return (
    <Menu {...props} className={"dropdown " + props.className} menuButton={<MenuButton className="small">
      <FaEllipsisH style={{ marginBottom: '-0.125em' }} />
    </MenuButton>}>
      {props.children}
    </Menu>
  )
  // const [isOpen, setIsOpen] = useState(false);
  // // const [selectedOption, setSelectedOption] = useState(null);
  // const dropdownRef = useRef(null);

  // useEffect(() => {
  //   const handleClickOutside = (event) => {
  //     if (dropdownRef.current && !dropdownRef?.current?.contains(event.target)) {
  //       setIsOpen(false);
  //     }
  //   };

  //   document.addEventListener('mousedown', handleClickOutside);

  //   return () => {
  //     document.removeEventListener('mousedown', handleClickOutside);
  //   };
  // }, [dropdownRef]);

  // const toggleDropdown = () => setIsOpen(!isOpen);

  // return (
  //   <div className="dropdown col" ref={dropdownRef}>
  //     <div className="dropdown-header row" onClick={toggleDropdown} style={displayStyle}>
  //       {display || <FaEllipsisH style={{ marginBottom: '-0.125em' }} />}
  //     </div>
  //     {isOpen && (
  //       <div className="dropdown-list col">
  //         {props.children}
  //       </div>
  //     )}
  //   </div>
  // );
}
