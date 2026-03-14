packer {
  required_plugins {
    qemu = {
      version = "~> 1"
      source  = "github.com/hashicorp/qemu"
    }
  }
}

variable "iso_path" {
  type        = string
  description = "Path to the Windows 11 ARM64 ISO"
}

variable "virtio_iso_path" {
  type        = string
  description = "Path to the virtio-win ISO (from Fedora)"
}

variable "output_directory" {
  type    = string
  default = "output"
}

variable "vm_name" {
  type    = string
  default = "windows-11-arm"
}

variable "disk_size" {
  type    = string
  default = "64G"
}

variable "memory" {
  type    = number
  default = 4096
}

variable "cpus" {
  type    = number
  default = 4
}

variable "winrm_username" {
  type    = string
  default = "test"
}

variable "winrm_password" {
  type    = string
  default = "TestPass123!"
}

source "qemu" "windows-11-arm" {
  qemu_binary = "qemu-system-aarch64"

  machine_type = "virt,highmem=on"
  accelerator  = "hvf"

  iso_url      = var.iso_path
  iso_checksum = "none"

  disk_size      = var.disk_size
  disk_interface = "virtio"
  format         = "qcow2"

  memory = var.memory
  cpus   = var.cpus

  # Headless with VNC — macOS QEMU doesn't support -display gtk
  headless         = true
  vnc_bind_address = "127.0.0.1"

  cd_files = ["autounattend.xml"]
  cd_label = "OEMDRV"

  # Additional QEMU args for aarch64 EFI boot.
  # Packer auto-generates: -drive (disk), -cdrom (ISO), -drive (cd_files ISO),
  #   -m, -machine, -smp, -vnc, -netdev, -device virtio-net, -name, -boot
  qemuargs = [
    ["-cpu", "host"],
    ["-bios", "/opt/homebrew/share/qemu/edk2-aarch64-code.fd"],
    ["-device", "ramfb"],
    ["-device", "qemu-xhci"],
    ["-device", "usb-kbd"],
    ["-device", "usb-tablet"],
    ["-drive", "file=${var.virtio_iso_path},media=cdrom,index=2"],
    # Override Packer's -boot flag — aarch64 virt doesn't support boot device lists,
    # but EFI firmware handles boot order automatically
    ["-boot", "menu=off"],
  ]

  communicator   = "winrm"
  winrm_username = var.winrm_username
  winrm_password = var.winrm_password
  winrm_timeout  = "60m"

  output_directory = var.output_directory
  vm_name          = var.vm_name

  shutdown_command = "shutdown /s /t 10 /f"
  shutdown_timeout = "5m"
}

build {
  sources = ["source.qemu.windows-11-arm"]

  provisioner "file" {
    source      = "scripts/provision.ps1"
    destination = "C:\\Windows\\Temp\\provision.ps1"
  }

  provisioner "powershell" {
    inline = [
      "Set-ExecutionPolicy RemoteSigned -Force",
      "& C:\\Windows\\Temp\\provision.ps1",
    ]
  }
}
