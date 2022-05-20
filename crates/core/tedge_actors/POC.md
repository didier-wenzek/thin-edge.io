```
 ┌─────────────┐               ┌──────────────────────────────────────────────┐                 ┌──────────────────────────────────────────────┐
 │             │ MqttMessage   │                                              │    SMRequest    │                                              │
 │ MQTT        ├───────────────► C8Y                                          ├─────────────────►  SM                                          │
 │             │               │                                              │                 │                                              │
 │             ◄───────────────┤                                              ◄─────────────────┤                                              │
 │             │  MqttMessage  │                                              │    SMResponse   │                                              │
 │             │               │                                              │                 │                                              │
 └─────────────┘               └───────▲───────────────────────────────▲──────┘                 └────┬───▲────────────────────────────┬───▲────┘
                                       │                               │                             │   │                            │   │
                                       │                               │                             │   │                            │   │
                           Measurement │                   Measurement │                   SMRequest │   │SMResponse         SMRequest│   │ SMResponse
                                       │                               │                             │   │                            │   │
                               ┌───────┴─────┐                 ┌───────┴─────┐                  ┌────▼───┴────┐                  ┌────▼───┴────┐
                               │             │                 │             │                  │             │                  │             │
                               │ Collectd    │                 │ ThinEdgeJSON│                  │  Apt        │                  │ Apama       │
                               │             │                 │             │                  │             │                  │             │
                               │             │                 │             │                  │             │                  │             │
                               │             │                 │             │                  │             │                  │             │
                               │             │                 │             │                  │             │                  │             │       
                               └───────▲─────┘                 └───────▲─────┘                  └─────────────┘                  └─────────────┘       
                                       │                               │
                                       │ MqttMessage                   │ MqttMessage                              
                                       │                               │
                               ┌───────┴─────┐                 ┌───────┴─────┐
                               │             │                 │             │
                               │ MQTT        │                 │ MQTT        │
                               │             │                 │             │
                               │             │                 │             │
                               │             │                 │             │     
                               │             │                 │             │
                               └─────────────┘                 └─────────────┘
```


```
                 ┌───────────────────────┐     ┌───────────────────────┐    ┌───────────────────────┐
                 │"plugin_c8y"           │     │"plugin_sm"            │    │"plugin_apt"           │
                 │                       ├─────►                       ◄────┤                       │
                 │                       │     │ SMRequest             │    │                       │
                 │                       │     │                       │    │                       │
                 │                       │     │ SMResponse            │    │                       │
                 └───┬───────────────┬───┘     └────────────────▲──────┘    └───────────────────────┘
                     │               │                          │
                     │               │                          │           ┌───────────────────────┐
                     │               │                          │           │"plugin_apama"         │
                     │               │                          └───────────┤                       │
                     │               │                                      │                       │
                     │               │                                      │                       │
                     │               │                                      │                       │
  ┌──────────────────▼────┐     ┌────▼──────────────────┐                   └───────────────────────┘
  │"plugin_mqtt"          │     │"plugin_telemetry"     │
  │                       │     │                       │
  │ MqttMessage           │     │ Measurement           │
  │                       │     │                       │
  │                       │     │                       │
  └───▲────────▲──────────┘     └────────────▲───▲──────┘
      │        │                             │   │
      │   ┌────┼─────────────────────────────┘   │                         ▼
      │   │    └─────────────────────┐           │
      │   │                          │           │
  ┌───┴───┴───────────────┐     ┌────┴───────────┴──────┐
  │"plugin_thinedge_json" │     │"plugin_collectd"      │
  │                       │     │                       │
  │                       │     │                       │
  │                       │     │                       │
  │                       │     │                       │ 
  └───────────────────────┘     └───────────────────────┘

```






```