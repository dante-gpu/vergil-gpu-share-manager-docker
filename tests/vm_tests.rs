// Virtual Machine Test Suite - Because untested code is like Schrödinger's cat! 🐱💻

use anyhow::{Context, Result};
use gpu_share_vm_manager::core::{LibvirtManager, vm::VMConfig};
use std::path::PathBuf;
// use tracing::{info, warn};
use uuid::Uuid;

#[derive(Clone)]
struct LibvirtManagerWrapper(LibvirtManager);

impl LibvirtManagerWrapper {
    fn new() -> Result<Self> {
        LibvirtManager::new().map(Self)
    }
}

// Test setup: Creates a unique VM configuration to avoid conflicts
fn test_vm_config() -> VMConfig {
    let uuid = Uuid::new_v4();
    VMConfig {
        name: format!("test-vm-{}", uuid),
        memory_kb: 1_048_576, // 1GB
        vcpus: 2,
        disk_path: PathBuf::from(format!("/var/lib/gpu-share/images/test-{}.qcow2", uuid)),
        disk_size_gb: 10,
        gpu_passthrough: None,
    }
}

// GPU test config
async fn create_gpu_vm_config() -> VMConfig {
    VMConfig {
        name: "gpu-test-vm".into(),
        memory_kb: 8 * 1024 * 1024,
        vcpus: 4,
        disk_path: PathBuf::from("/var/lib/libvirt/images/gpu-test.qcow2"),
        disk_size_gb: 40,
        gpu_passthrough: Some("0000:01:00.0".into()),
    }
}

// Big Scale VM Test
fn create_large_vm_config() -> VMConfig {
    VMConfig {
        name: "large-test-vm".into(),
        memory_kb: 16 * 1024 * 1024,
        vcpus: 8,
        disk_path: PathBuf::from("/var/lib/libvirt/images/large-test.qcow2"),
        disk_size_gb: 100,
        gpu_passthrough: None,
    }
}

// Minimum Resources Test
fn create_minimal_vm_config() -> VMConfig {
    VMConfig {
        name: "minimal-test-vm".into(),
        memory_kb: 512 * 1024,
        vcpus: 1,
        disk_path: PathBuf::from("/var/lib/libvirt/images/minimal-test.qcow2"),
        disk_size_gb: 10,
        gpu_passthrough: None,
    }
}

// VM Lifecycle Test: Creation → Start → Stop → Delete
#[tokio::test]
async fn test_full_vm_lifecycle() -> Result<()> {
    let libvirt = LibvirtManagerWrapper::new()?;
    let config = test_vm_config();
    
    // Phase 1: Create the VM
    let vm = libvirt.0.create_vm(&config)
        .await
        .context("Failed to create VM")?;
    
    assert_eq!(vm.get_name()?, config.name);
    assert!(!vm.is_active()?, "VM should be initially stopped");

    // Phase 2: Start the VM
    vm.create()?;
    assert!(vm.is_active()?, "VM should be running after start");

    // Phase 3: Stop the VM
    vm.destroy()?;
    assert!(!vm.is_active()?, "VM should be stopped after destroy");

    // Phase 4: Delete the VM
    vm.undefine()?;
    
    // Verify deletion
    let exists = libvirt.0.lookup_domain(&config.name).is_ok();
    assert!(!exists, "VM should be deleted");

    Ok(())
}

// Stress Test: Create multiple VMs simultaneously
#[tokio::test]
async fn test_concurrent_vm_creation() -> Result<()> {
    let libvirt = LibvirtManagerWrapper::new()?;
    let mut handles = vec![];
    
    // Spawn 5 concurrent VM creations
    for i in 0..5 {
        let cloned = libvirt.clone();
        let config = VMConfig {
            name: format!("stress-test-vm-{}", i),
            memory_kb: 524_288, // 512MB
            vcpus: 1,
            disk_path: PathBuf::from(format!("/var/lib/gpu-share/images/stress-{}.qcow2", i)),
            disk_size_gb: 5,
            gpu_passthrough: None,
        };
        
        handles.push(tokio::spawn(async move {
            cloned.0.create_vm(&config).await
        }));
    }

    // Verify all creations succeeded
    for handle in handles {
        let vm = handle.await??;
        assert!(vm.get_name().is_ok(), "VM should have valid name");
        vm.destroy()?;
        vm.undefine()?;
    }

    Ok(())
}

// Error Case Test: Invalid VM configurations
#[tokio::test]
async fn test_invalid_vm_configurations() -> Result<()> {
    let libvirt = LibvirtManagerWrapper::new()?;
    
    // Test 1: Insufficient memory
    let config = VMConfig {
        name: "invalid-memory".into(),
        memory_kb: 1024, // Ridiculously low
        vcpus: 2,
        disk_path: PathBuf::from("/invalid/path.qcow2"),
        disk_size_gb: 10,
        gpu_passthrough: None,
    };
    
    let result = libvirt.0.create_vm(&config).await;
    assert!(result.is_err(), "Should reject insufficient memory");

    // Test 2: Invalid disk path
    let config = VMConfig {
        name: "invalid-disk".into(),
        memory_kb: 1_048_576,
        vcpus: 2,
        disk_path: PathBuf::from("/dev/null"), // Invalid disk image
        disk_size_gb: 10,
        gpu_passthrough: None,
    };
    
    let result = libvirt.0.create_vm(&config).await;
    assert!(result.is_err(), "Should reject invalid disk path");

    Ok(())
}

// State Transition Test: Start → Reboot → Stop
#[tokio::test]
async fn test_vm_state_transitions() -> Result<()> {
    let libvirt = LibvirtManagerWrapper::new()?;
    let config = test_vm_config();
    let vm = libvirt.0.create_vm(&config).await?;

    // Cold start
    vm.create()?;
    assert!(vm.is_active()?, "VM should be running");

    // Reboot
    vm.reboot(0)?;
    assert!(vm.is_active()?, "VM should stay running after reboot");

    // Graceful shutdown
    vm.shutdown()?;
    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
    assert!(!vm.is_active()?, "VM should shutdown gracefully");

    vm.undefine()?;
    Ok(())
}

// Snapshot Test: Create → Snapshot → Restore
#[tokio::test]
async fn test_vm_snapshots() -> Result<()> {
    let libvirt = LibvirtManagerWrapper::new()?;
    let config = test_vm_config();
    let vm = libvirt.0.create_vm(&config).await?;
    vm.create()?;

    // Create snapshot with proper XML structure
    let snapshot_xml = r#"
    <domainsnapshot>
        <name>test-snapshot</name>
        <description>Initial state</description>
        <memory snapshot='no'/>
    </domainsnapshot>"#;
    
    // Create snapshot and verify
    let snapshot = vm.snapshot_create_xml(snapshot_xml, 0)
        .context("Failed to create snapshot")?;
    assert_eq!(snapshot.get_name()?, "test-snapshot");

    // Revert to snapshot
    vm.snapshot_revert(snapshot, 0)
        .context("Failed to revert snapshot")?;

    // Cleanup snapshot
    let current_snapshot = vm.snapshot_current(0)?;
    current_snapshot.delete(0)?;

    vm.destroy()?;
    vm.undefine()?;
    Ok(())
}

// Resource Validation Test: CPU/Memory allocation
#[tokio::test]
async fn test_resource_allocation() -> Result<()> {
    let libvirt = LibvirtManagerWrapper::new()?;
    let config = test_vm_config();
    
    // Create VM with specific resources
    let vm = libvirt.0.create_vm(&config)
        .await
        .context("Failed to create VM for resource test")?;

    // Validate memory allocation
    let info = vm.get_info()?;
    assert_eq!(
        info.memory as u64, 
        config.memory_kb * 1024,  // Convert KiB to bytes
        "Memory allocation mismatch"
    );

    // Validate vCPU allocation
    assert_eq!(
        info.nr_virt_cpu as u32,
        config.vcpus,
        "vCPU allocation mismatch"
    );

    // Cleanup
    vm.destroy()?;
    vm.undefine()?;
    
    Ok(())
}

// Network Configuration Test: Validate network interfaces and connectivity
#[tokio::test]
async fn test_vm_network_configuration() -> Result<()> {
    let libvirt = LibvirtManagerWrapper::new()?;
    let config = test_vm_config();
    
    // Create VM with network configuration
    let vm = libvirt.0.create_vm(&config)
        .await
        .context("Failed to create VM for network test")?;
    
    vm.create()?;
    
    // Validate network interfaces
    let interfaces = vm.get_interfaces()?;
    assert!(!interfaces.is_empty(), "VM should have at least one network interface");
    
    // Basic connectivity check (ping gateway)
    let active_iface = interfaces.first().unwrap();
    let ping_result = vm.execute_command(&format!("ping -c 3 {}", active_iface.gateway)).await;
    assert!(ping_result.is_ok(), "VM should have network connectivity");
    
    // Cleanup
    vm.destroy()?;
    vm.undefine()?;
    Ok(())
}

// Negative Test: Duplicate VM creation and error handling
#[tokio::test]
async fn test_duplicate_vm_creation() -> Result<()> {
    let libvirt = LibvirtManagerWrapper::new()?;
    let config = test_vm_config();
    
    // First creation should succeed
    let vm1 = libvirt.0.create_vm(&config)
        .await
        .context("First VM creation should succeed")?;
    
    // Second creation with same config should fail
    let result = libvirt.0.create_vm(&config).await;
    assert!(
        result.is_err(),
        "Should return error when creating duplicate VM"
    );
    
    // Verify error type
    if let Err(e) = result {
        assert!(
            e.to_string().contains("already exists"),
            "Error should indicate duplicate VM"
        );
    }
    
    // Cleanup
    vm1.destroy()?;
    vm1.undefine()?;
    Ok(())
}