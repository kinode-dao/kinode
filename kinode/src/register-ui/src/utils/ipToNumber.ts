export function ipToNumber(ip: string) {
    const octets = ip.split('.'); // Split the IP by the dot delimiter
    if (octets.length !== 4) {
      throw new Error('Invalid IP address');
    }
    
    let ipNum = 0;
    for (let i = 0; i < 4; i++) {
      ipNum <<= 8; // Shift existing bits 8 positions to the left
      ipNum += parseInt(octets[i], 10); // Parse octet to base 10 integer and add to ipNum
    }
    
    return ipNum >>> 0; // Convert to unsigned 32-bit integer
  }