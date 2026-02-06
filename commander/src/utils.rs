use std::net::Ipv6Addr;

/// Calculate a unique IPv6 address by adding an offset to a base prefix
pub fn calculate_ipv6(prefix: &str, index: u32) -> anyhow::Result<String> {
    let base_addr: Ipv6Addr = prefix.parse()
        .map_err(|_| anyhow::anyhow!("Invalid IPv6 prefix: {}", prefix))?;
    
    // Convert to u128 for bitwise math
    let base_u128 = u128::from(base_addr);
    
    // Add the index to the address
    let new_u128 = base_u128.checked_add(index as u128)
        .ok_or_else(|| anyhow::anyhow!("IPv6 address overflow for index {}", index))?;
    
    Ok(Ipv6Addr::from(new_u128).to_string())
}
