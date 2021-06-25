Deployment
==========

Fresh Installation
------------------


Common Installation Steps
*************************

Both fxrecorder and fxrunner instances require running the following steps:

1. Install PowerShell 7+

   PowerShell can be downloaded from `GitHub <ghpowershell_>`_. The default
   options are sufficient.

2. Install and configure OpenSSH server.

   SSH is used for remote management of fxrecorder and fxrunner. To install on
   Windows, open an elevated PowerShell window and run:

   .. code-block:: ps1

      $cap = Get-WindowsCapability -Online | ? Name -Like "OpenSSH.Server*"
      Add-WindowsCapability -Online -Name $cap.Name

   Windows will download and install OpenSSH server.

3. Configure OpenSSH

   The default configuration of OpenSSH does not allow PowerShell remoting.

   First, start and stop the OpenSSH server to have it generate the default
   configuration:

   .. code-block:: ps1

      Set-Service -Name sshd -Status Running
      Set-Service -Name sshd -Status Stopped

   Then, open the configuration file at :file:`C:\\ProgramData\\ssh\\sshd_config` and
   add the following

   .. code-block::

      Subsystem	powershell	pwsh.exe -sshs -NoLogo

   Finally, enable and start the sshd service:

   .. code-block:: ps1

      Set-Service -Name sshd -StartupType Automatic -Status Running

4. Do platform-specific configuration (`fxrecorder <install_fxrecorder_>`_ or
   `fxrunner <install_fxrunner_>`_).

.. _install_fxrecorder:

fxrecorder
**********

1. Install ImageMagick and FFmpeg.

   fxrecoder additionally requires an installation of ImageMagick 6.9 and ffmpeg
   4.2. Download the `latest Windows binary release <imagemagick_>`_ and run the
   installer. Make sure to also check "Install FFmpeg".

2. Run the deployment script.

   This script will copy all management modules and scripts to the host, as well
   as build and deploy fxrecorder with its configuration.

   .. code-block:: ps1

      .\contrib\Deploy.ps1 -HostName $hostname -UserName fxrecorder -MachineType recorder

3. Run the installation script.

   The installation script is run on the remote host over SSH. Run the following
   to authenticate to the machine and run the script:

   .. code-block:: ps1

     Enter-PSSession -UserName fxrecorder $hostname
     Install-FxRecorder.ps1
     Exit-PSSession

   The installation script will handle download and installation of all other
   dependencies. It will prompt you for the Taskcluster access token, client ID,
   and worker ID.


.. _install_fxrunner:

fxrunner
********

1. Run the deployment script.

   This script will copy all management modules and scripts to the host, as well
   as build and deploy fxrecorder with its configuration.

   .. code-block:: ps1

      .\contrib\Deploy.ps1 -HostName $hostname -UserName fxrecorder -MachineType runner

4. Run the installation script

   The installation script is run on the remote host over SSH. Run the following
   to authenticate to the machine and run the script:

   .. code-block:: ps1

     Enter-PSSession -UserName fxrunner $hostname
     .\PowerShell\Scripts\Install-FxRunner.ps1
     Exit-PSSession

Updating Existing Deployments
-----------------------------

.. _update_fxrecorder:

fxrecorder
**********

To update an existing deployment of fxrecorder, run the following PowerShell code:

.. code-block:: ps1

   $session = New-PSSession -UserName fxrecorder $hostname
   Invoke-Command -Session $session -ScriptBlock { Disable-FxRecorder.ps1 }
   .\Contrib\Deploy -HostName $hostname -UserName fxrecorder -MachineType recorder
   Invoke-Command -Session $session -ScriptBlock { Enable-FxRecorder.ps1 }
   Remove-PSSession $session

.. _update_fxrunner:

fxrunner
********

To update an existing deployment of fxrunner, run the following PowerShell code:

.. code-block:: ps1

   $session = New-PSSession -UserName fxrunner HOSTNAME
   Invoke-Command -Session $session -ScriptBlock { Disable-FxRunner.ps1 }
   .\contrib\Deploy -HostName HOSTNAME -UserName fxrunner -MachineType runner
   Invoke-Command -Session $session -ScriptBlock { Enable-FxRunner.ps1 }
   Remove-PSSession $session


.. _ghpowershell:  https://github.com/PowerShell/PowerShell/releases/latest/
.. _imagemagick: https://legacy.imagemagick.org/script/download.php#windows
