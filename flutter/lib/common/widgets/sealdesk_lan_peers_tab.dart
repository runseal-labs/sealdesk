import 'package:flutter/material.dart';

import '../../common.dart';
import '../sealdesk_connect.dart';
import '../sealdesk_connect_target.dart';
import 'sealdesk_connect_field.dart';

class SealDeskLanPeersTab extends StatefulWidget {
  const SealDeskLanPeersTab({super.key});

  @override
  State<SealDeskLanPeersTab> createState() => _SealDeskLanPeersTabState();
}

class _SealDeskLanPeersTabState extends State<SealDeskLanPeersTab> {
  @override
  void initState() {
    super.initState();
    SealDeskLanDirectory.instance.retain();
  }

  @override
  void dispose() {
    SealDeskLanDirectory.instance.release();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return AnimatedBuilder(
      animation: SealDeskLanDirectory.instance,
      builder: (context, _) {
        final devices = SealDeskLanDirectory.instance.devices;
        if (devices.isEmpty) {
          return Center(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                Icon(Icons.radar,
                    key: const ValueKey('sealdesk-lan-search'),
                    size: 36,
                    color: Theme.of(context).hintColor),
                const SizedBox(height: 8),
                Text(translate(kSealDeskSearching)),
              ],
            ),
          );
        }
        return ListView.builder(
          key: const ValueKey('sealdesk-lan-list'),
          itemCount: devices.length,
          itemBuilder: (context, index) {
            final device = devices[index];
            return ListTile(
              key: ValueKey('sealdesk-lan-${device.address}'),
              dense: true,
              title: Text(device.name.isEmpty ? device.ip : device.name),
              subtitle: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(device.ip),
                  const SizedBox(height: 4),
                  SealDeskSecurityChip(encrypted: device.encrypted),
                ],
              ),
              isThreeLine: true,
              trailing: TextButton(
                key: ValueKey('sealdesk-lan-connect-${device.address}'),
                onPressed: () async {
                  if (!device.encrypted) {
                    await maybeShowSealDeskCompatHint(context);
                  }
                  if (!context.mounted) return;
                  await connectSealDeskTarget(
                    context,
                    SealDeskConnectTarget(
                      id: device.address,
                      kind: 'lan',
                      passwordKind: kSealDeskPasswordPermanent,
                    ),
                  );
                },
                child: Text(translate('Connect')),
              ),
            );
          },
        );
      },
    );
  }
}
