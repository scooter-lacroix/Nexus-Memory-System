#!/bin/bash
# Nexus Memory System Installation Script

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
INSTALL_DIR="/opt/nexus"
SERVICE_USER="nexus"
VENV_DIR="$INSTALL_DIR/venv"
DATA_DIR="$INSTALL_DIR/data"
LOG_DIR="$INSTALL_DIR/logs"

# Functions
print_status() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

print_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

print_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Check if running as root
if [[ $EUID -eq 0 ]]; then
   print_error "This script should not be run as root. Run as a regular user with sudo."
   exit 1
fi

print_status "Starting Nexus Memory System installation..."

# Check system requirements
print_status "Checking system requirements..."

if ! command -v python3 &> /dev/null; then
    print_error "Python 3 is required but not installed."
    exit 1
fi

PYTHON_VERSION=$(python3 -c 'import sys; print(".".join(map(str, sys.version_info[:2])))')
if [[ $(echo "$PYTHON_VERSION < 3.9" | bc -l) -eq 1 ]]; then
    print_error "Python 3.9 or higher is required. Found: $PYTHON_VERSION"
    exit 1
fi

if ! command -v pip3 &> /dev/null; then
    print_error "pip3 is required but not installed."
    exit 1
fi

print_success "System requirements met"

# Create directories
print_status "Creating installation directories..."
sudo mkdir -p "$INSTALL_DIR"
sudo mkdir -p "$DATA_DIR"
sudo mkdir -p "$LOG_DIR"
print_success "Directories created"

# Create service user
print_status "Creating nexus service user..."
if ! id "$SERVICE_USER" &>/dev/null; then
    sudo useradd -r -s /bin/false -d "$INSTALL_DIR" "$SERVICE_USER"
    print_success "User $SERVICE_USER created"
else
    print_warning "User $SERVICE_USER already exists"
fi

# Copy application files
print_status "Installing Nexus application files..."
TEMP_DIR=$(mktemp -d)
cp -r . "$TEMP_DIR/nexus"
sudo cp -r "$TEMP_DIR/nexus"/* "$INSTALL_DIR/"
rm -rf "$TEMP_DIR"
print_success "Application files copied"

# Set permissions
print_status "Setting permissions..."
sudo chown -R "$SERVICE_USER:$SERVICE_USER" "$INSTALL_DIR"
sudo chmod 755 "$INSTALL_DIR"
sudo chmod 755 "$INSTALL_DIR/nexus/agents/scripts"
print_success "Permissions set"

# Create virtual environment
print_status "Creating Python virtual environment..."
sudo -u "$SERVICE_USER" python3 -m venv "$VENV_DIR"
sudo -u "$SERVICE_USER" "$VENV_DIR/bin/pip" install --upgrade pip
print_success "Virtual environment created"

# Install dependencies
print_status "Installing Python dependencies..."
sudo -u "$SERVICE_USER" "$VENV_DIR/bin/pip" install -e "$INSTALL_DIR"
print_success "Dependencies installed"

# Initialize database
print_status "Initializing Nexus database..."
sudo -u "$SERVICE_USER" "$VENV_DIR/bin/nexus" init
print_success "Database initialized"

# Create systemd service
print_status "Creating systemd service..."
sudo cp "$INSTALL_DIR/scripts/nexus.service" /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable nexus
print_success "Systemd service created and enabled"

# Create configuration file
print_status "Creating configuration..."
if [[ ! -f "$INSTALL_DIR/.env" ]]; then
    sudo -u "$SERVICE_USER" cp "$INSTALL_DIR/.env.example" "$INSTALL_DIR/.env"
    print_success "Configuration created at $INSTALL_DIR/.env"
    print_warning "Review and modify the configuration file as needed"
else
    print_warning "Configuration file already exists"
fi

# Start service
print_status "Starting Nexus service..."
sudo systemctl start nexus

# Wait a moment for service to start
sleep 3

# Check service status
if sudo systemctl is-active --quiet nexus; then
    print_success "Nexus service is running"
else
    print_error "Nexus service failed to start"
    sudo systemctl status nexus
    exit 1
fi

# Create CLI symlink
print_status "Creating CLI symlink..."
sudo ln -sf "$VENV_DIR/bin/nexus" /usr/local/bin/nexus
print_success "CLI symlink created"

# Installation complete
print_success "Nexus Memory System installation complete!"
echo
print_status "Service Information:"
echo "  Status: $(sudo systemctl is-active nexus)"
echo "  Web UI: http://$(hostname -I | awk '{print $1}'):8768"
echo "  API: http://$(hostname -I | awk '{print $1}'):8767"
echo "  Data directory: $DATA_DIR"
echo "  Log directory: $LOG_DIR"
echo
print_status "Useful Commands:"
echo "  nexus status          - Check system status"
echo "  nexus logs            - View service logs"
echo "  sudo systemctl status nexus - Check service status"
echo "  sudo journalctl -u nexus -f - Follow service logs"
echo
print_status "Configuration:"
echo "  Edit configuration: sudo nano $INSTALL_DIR/.env"
echo "  Restart service: sudo systemctl restart nexus"
echo
print_success "Thank you for installing Nexus Memory System!"