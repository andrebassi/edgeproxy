import type {SidebarsConfig} from '@docusaurus/plugin-content-docs';

const sidebars: SidebarsConfig = {
  tutorialSidebar: [
    'intro',
    'getting-started',
    'architecture',
    'wireguard',
    'internals/load-balancer',
    'internals/client-affinity',
    'internals/node-management',
    'deployment/docker',
    'deployment/flyio',
    {
      type: 'category',
      label: 'AWS',
      items: [
        'deployment/aws',
        {
          type: 'category',
          label: 'RDS',
          items: [
            'deployment/aws-rds-wireguard',
            'deployment/rds-benchmark',
          ],
        },
      ],
    },
    'deployment/gcp',
    {
      type: 'category',
      label: 'Configuration',
      link: {
        type: 'doc',
        id: 'configuration/index',
      },
      items: [
        'configuration/environment-variables',
        'configuration/database-schema',
        'configuration/dns-server',
        'configuration/auto-discovery-api',
        'configuration/corrosion',
        'configuration/infrastructure',
      ],
    },
    'testing',
    'performance',
    'benchmark',
    {
      type: 'category',
      label: 'Roadmap',
      link: {
        type: 'doc',
        id: 'roadmap/index',
      },
      items: [
        'roadmap/phase-1-ipv6',
        'roadmap/phase-2-anycast-bgp',
        'roadmap/phase-3-health-checks',
      ],
    },
  ],
};

export default sidebars;
