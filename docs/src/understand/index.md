# Building smart IIoT agents with thin-edge

Thin-edge is an open-source IIoT development toolbox based on a versatile set of ready-to-use software components
that can be easily combined with extensions specific to an equipment, an application or a cloud end-point.

Thin-edge is designed to ease the development of smart IIoT agents by combining both out-of-the-box functionality with simple yet flexible extension points. The extension points enable custom functionality to be added in a modular fashion reducing the development efforts significantly.

The agents typically run on the edge, at the frontier between IT cloud computing and OT industrial equipment. They act as gateways between the cloud and the devices embedded into smart equipment, machines or plants.

The main functions and challenges are to:
- establish a secure and reliable connection from the cloud to a fleet of smart equipment
- provide a uniform way to monitor and control these assets despite the diversity of hardware and protocols
- collect telemetry and monitoring data from the various sensors and processes running on the devices
- process data with local analytics tools and push the relevant subset to the cloud
- monitor, configure and update agents and the attached devices from the cloud.

To implement these functions, thin-edge proposes to design an IIoT agent using a combination of software components,
which are deployed on the main agent device as well as the set of interconnected embedded devices that form the equipment.

A typical thin-edge setup consists of the following components:
- a local MQTT broker, currently [mosquitto](https://mosquitto.org/),
  that is used as a message bus between all the components of the equipment
- an MQTT bridge connection between the local message bus and the remote cloud end-point
- thin-edge (out-of-the-box) device management services which provide features such as; monitoring, configuration and software management
- equipment-specific services that interact with the hardware that make the equipment,
  abstracting the various protocol, publishing on the MQTT bus collected data and forwarding operation requests
- a cloud-specific service that maps the messages exchanged on the local bus with messages sent to or received from the cloud

The first point to note is that all these software components can be provided by independent vendors:
by the thin-edge open-source project, by the equipment maker, by the IoT application developer
or even by cloud providers, hardware manufacturers and protocol implementors. 
Their interoperability is based on:
- ubiquitous protocols: JSON over MQTT and HTTP,
- dynamic and loose inter-process communication with no constraint on programming tools nor software placement,
- domain-specific APIs that can be used independently:
  for measurements, events, alarms, configurations, software updates, firmware updates,   
- various extension points: for child devices, services, operations and clouds.

The second key point is that thin-edge not only defines a set of APIs
but also provides a set of ready-to-use software components implementing the associated features.
Out-of-the-box thin-edge supports telemetry and device management features on the main devices and child devices.
These features are implemented by composable software components that:
- can be freely adapted, combined, removed or replaced
- provide the foundation to start the development of an agent with sensible defaults
- on top of which can be incrementally implemented the specificities required by smart IIoT agents

The flexibility of thin-edge means that it can be used a different levels.
- As a beginner, the simpler is to use thin-edge as a pre-assembled agent,
  ready to be installed on a device and configured for a trial cloud account.
  The [getting started guide](../start/index.md) gives a taste of what can be done with thin-edge out-of-the-box
- As a cloud operator, no direct access to a device is required, expect for occasional troubleshooting,
  as most of the operations can be done remotely.
  However, being able to operate directly a device gives the required understanding
  to administrate a fleet of thin-edge devices with confidence
- As a __device operator__, be prepared to operate a device that is not running the pre-assembled thin-edge agent,
  but an agent specifically designed for your equipment and application.
  Among all the [available features](../operate/index.md),
  some might have been configured differently, disabled, extended, replaced or even added
- As an __agent developer__, the nature of thin-edge let you
  start the design of an agent with the pre-assembled agent
  and to incrementally [configure, adapt and extend](../extend/index.md) the agent
  to meet the requirements of the equipment and application.
  Part of this work can be to implement software components
  that interact with thin-edge through its JSON API over MQTT and HTTP
  and are to be deployed on the main devices and the attached child devices
- As a contributor, you can [extend thin-edge using its Rust API](../contribute/index.md),
  when the loosing coupling of MQTT and HTTP is not the more appropriate approach