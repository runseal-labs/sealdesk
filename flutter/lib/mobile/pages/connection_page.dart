import 'dart:async';

import 'package:auto_size_text_field/auto_size_text_field.dart';
import 'package:flutter/material.dart';
import 'package:sealdesk/common/formatter/id_formatter.dart';
import 'package:sealdesk/common/widgets/connection_page_title.dart';
import 'package:sealdesk/models/state_model.dart';
import 'package:get/get.dart';
import 'package:provider/provider.dart';
import 'package:sealdesk/models/peer_model.dart';

import '../../common.dart';
import '../../common/sealdesk_connect_target.dart';
import '../../common/widgets/peer_tab_page.dart';
import '../../common/widgets/sealdesk_connect_field.dart';
import '../../common/widgets/autocomplete.dart';
import '../../consts.dart';
import '../../models/model.dart';
import '../../models/platform_model.dart';
import 'home_page.dart';
import '../sealdesk_scan.dart';
import '../widgets/android_update_banner.dart';

/// Connection page for connecting to a remote peer.
class ConnectionPage extends StatefulWidget implements PageShape {
  ConnectionPage({Key? key, required this.appBarActions}) : super(key: key);

  @override
  final icon = const Icon(Icons.connected_tv);

  @override
  final title = translate("Connection");

  @override
  final List<Widget> appBarActions;

  @override
  State<ConnectionPage> createState() => _ConnectionPageState();
}

/// State for the connection page.
class _ConnectionPageState extends State<ConnectionPage> {
  /// Controller for the id input bar.
  final _idController = IDTextEditingController();
  final RxBool _idEmpty = true.obs;

  final FocusNode _idFocusNode = FocusNode();
  final TextEditingController _idEditingController = TextEditingController();

  final AllPeersLoader _allPeersLoader = AllPeersLoader();

  StreamSubscription? _uniLinksSubscription;

  // https://github.com/flutter/flutter/issues/157244
  Iterable<Peer> _autocompleteOpts = [];

  _ConnectionPageState() {
    if (!isWeb) _uniLinksSubscription = listenUniLinks();
    _idController.addListener(() {
      _idEmpty.value = _idController.text.isEmpty;
    });
    Get.put<IDTextEditingController>(_idController);
  }

  @override
  void initState() {
    super.initState();
    _allPeersLoader.init(setState);
    _idFocusNode.addListener(onFocusChanged);
    if (!isSealDeskApp() && _idController.text.isEmpty) {
      WidgetsBinding.instance.addPostFrameCallback((_) async {
        final lastRemoteId = await bind.mainGetLastRemoteId();
        if (lastRemoteId != _idController.id) {
          setState(() {
            _idController.id = lastRemoteId;
          });
        }
      });
    }
    _idEditingController.addListener(() {
      _idController.id = _idEditingController.text;
    });
    Get.put<TextEditingController>(_idEditingController);
  }

  @override
  Widget build(BuildContext context) {
    Provider.of<FfiModel>(context);
    if (isSealDeskApp()) {
      return Column(
        children: [
          const AndroidUpdateBanner(),
          SealDeskConnectField(
            controller: _idEditingController,
            onScan: () => scanAndConnectSealDesk(context),
          ),
          const Expanded(child: PeerTabPage()),
        ],
      ).marginOnly(top: 2, left: 10, right: 10);
    }
    return CustomScrollView(
      slivers: [
        SliverList(
            delegate: SliverChildListDelegate([
          const AndroidUpdateBanner(),
          _buildRemoteIDTextField(),
        ])),
        SliverFillRemaining(
          hasScrollBody: true,
          child: PeerTabPage(),
        )
      ],
    ).marginOnly(top: 2, left: 10, right: 10);
  }

  /// Callback for the connect button.
  /// Connects to the selected peer.
  void onConnect() {
    var id = _idController.id;
    connect(context, id);
  }

  void onFocusChanged() {
    _idEmpty.value = _idEditingController.text.isEmpty;
    if (_idFocusNode.hasFocus) {
      if (_allPeersLoader.needLoad) {
        _allPeersLoader.getAllPeers();
      }

      final textLength = _idEditingController.value.text.length;
      // Select all to facilitate removing text, just following the behavior of address input of chrome.
      _idEditingController.selection =
          TextSelection(baseOffset: 0, extentOffset: textLength);
    }
  }

  /// UI for the remote ID TextField.
  /// Search for a peer and connect to it if the id exists.
  Widget _buildRemoteIDTextField() {
    final w = SizedBox(
      height: 84,
      child: Padding(
        padding: const EdgeInsets.symmetric(vertical: 8, horizontal: 2),
        child: Ink(
          decoration: BoxDecoration(
            color: Theme.of(context).cardColor,
            borderRadius: BorderRadius.all(Radius.circular(13)),
          ),
          child: Row(
            children: <Widget>[
              Expanded(
                child: Container(
                  padding: const EdgeInsets.only(left: 16, right: 16),
                  child: RawAutocomplete<Peer>(
                    optionsBuilder: (TextEditingValue textEditingValue) {
                      if (textEditingValue.text == '') {
                        _autocompleteOpts = const Iterable<Peer>.empty();
                      } else if (_allPeersLoader.peers.isEmpty &&
                          !_allPeersLoader.isPeersLoaded) {
                        Peer emptyPeer = Peer(
                          id: '',
                          username: '',
                          hostname: '',
                          alias: '',
                          platform: '',
                          tags: [],
                          hash: '',
                          password: '',
                          rdpPort: '',
                          rdpUsername: '',
                          loginName: '',
                          device_group_name: '',
                          note: '',
                        );
                        _autocompleteOpts = [emptyPeer];
                      } else {
                        String textWithoutSpaces =
                            textEditingValue.text.replaceAll(" ", "");
                        if (int.tryParse(textWithoutSpaces) != null) {
                          textEditingValue = TextEditingValue(
                            text: textWithoutSpaces,
                            selection: textEditingValue.selection,
                          );
                        }
                        String textToFind = textEditingValue.text.toLowerCase();

                        _autocompleteOpts = _allPeersLoader.peers
                            .where((peer) =>
                                peer.id.toLowerCase().contains(textToFind) ||
                                peer.username
                                    .toLowerCase()
                                    .contains(textToFind) ||
                                peer.hostname
                                    .toLowerCase()
                                    .contains(textToFind) ||
                                peer.alias.toLowerCase().contains(textToFind))
                            .toList();
                      }
                      return _autocompleteOpts;
                    },
                    focusNode: _idFocusNode,
                    textEditingController: _idEditingController,
                    fieldViewBuilder: (BuildContext context,
                        TextEditingController fieldTextEditingController,
                        FocusNode fieldFocusNode,
                        VoidCallback onFieldSubmitted) {
                      updateTextAndPreserveSelection(
                          fieldTextEditingController, _idController.text);
                      return AutoSizeTextField(
                        controller: fieldTextEditingController,
                        focusNode: fieldFocusNode,
                        minFontSize: 18,
                        autocorrect: false,
                        enableSuggestions: false,
                        keyboardType: TextInputType.visiblePassword,
                        // keyboardType: TextInputType.number,
                        onChanged: (String text) {
                          _idController.id = text;
                        },
                        style: const TextStyle(
                          fontFamily: 'WorkSans',
                          fontWeight: FontWeight.bold,
                          fontSize: 30,
                          color: MyTheme.idColor,
                        ),
                        decoration: InputDecoration(
                          labelText: translate(
                              bind.mainGetAppNameSync() == 'SealDesk'
                                  ? 'Connect to a device'
                                  : 'Remote ID'),
                          // hintText: 'Enter your remote ID',
                          border: InputBorder.none,
                          helperStyle: const TextStyle(
                            fontWeight: FontWeight.bold,
                            fontSize: 16,
                            color: MyTheme.darkGray,
                          ),
                          labelStyle: const TextStyle(
                            fontWeight: FontWeight.w600,
                            fontSize: 16,
                            letterSpacing: 0.2,
                            color: MyTheme.darkGray,
                          ),
                        ),
                        inputFormatters: [IDTextInputFormatter()],
                        onSubmitted: (_) {
                          onConnect();
                        },
                      );
                    },
                    onSelected: (option) {
                      setState(() {
                        _idController.id = option.id;
                        FocusScope.of(context).unfocus();
                      });
                    },
                    optionsViewBuilder: (BuildContext context,
                        AutocompleteOnSelected<Peer> onSelected,
                        Iterable<Peer> options) {
                      options = _autocompleteOpts;
                      double maxHeight = options.length * 50;
                      if (options.length == 1) {
                        maxHeight = 52;
                      } else if (options.length == 3) {
                        maxHeight = 146;
                      } else if (options.length == 4) {
                        maxHeight = 193;
                      }
                      maxHeight = maxHeight.clamp(0, 200);
                      return Align(
                          alignment: Alignment.topLeft,
                          child: Container(
                              decoration: BoxDecoration(
                                boxShadow: [
                                  BoxShadow(
                                    color: Colors.black.withOpacity(0.3),
                                    blurRadius: 5,
                                    spreadRadius: 1,
                                  ),
                                ],
                              ),
                              child: ClipRRect(
                                  borderRadius: BorderRadius.circular(5),
                                  child: Material(
                                      elevation: 4,
                                      child: ConstrainedBox(
                                          constraints: BoxConstraints(
                                            maxHeight: maxHeight,
                                            maxWidth: 320,
                                          ),
                                          child: _allPeersLoader
                                                      .peers.isEmpty &&
                                                  !_allPeersLoader.isPeersLoaded
                                              ? Container(
                                                  height: 80,
                                                  child: Center(
                                                      child:
                                                          CircularProgressIndicator(
                                                    strokeWidth: 2,
                                                  )))
                                              : ListView(
                                                  padding:
                                                      EdgeInsets.only(top: 5),
                                                  children: options
                                                      .map((peer) =>
                                                          AutocompletePeerTile(
                                                              onSelect: () =>
                                                                  onSelected(
                                                                      peer),
                                                              peer: peer))
                                                      .toList(),
                                                ))))));
                    },
                  ),
                ),
              ),
              Obx(() => Offstage(
                    offstage: _idEmpty.value,
                    child: IconButton(
                        onPressed: () {
                          setState(() {
                            _idController.clear();
                          });
                        },
                        icon: Icon(Icons.clear, color: MyTheme.darkGray)),
                  )),
              SizedBox(
                width: 60,
                height: 60,
                child: IconButton(
                  icon: const Icon(Icons.arrow_forward,
                      color: MyTheme.darkGray, size: 45),
                  onPressed: onConnect,
                ),
              ),
            ],
          ),
        ),
      ),
    );
    final child = Column(children: [
      if (isWebDesktop)
        getConnectionPageTitle(context, true)
            .marginOnly(bottom: 10, top: 15, left: 12),
      w
    ]);
    return Align(
        alignment: Alignment.topCenter,
        child: Container(constraints: kMobilePageConstraints, child: child));
  }

  @override
  void dispose() {
    _uniLinksSubscription?.cancel();
    _idController.dispose();
    _idFocusNode.removeListener(onFocusChanged);
    _allPeersLoader.clear();
    _idFocusNode.dispose();
    _idEditingController.dispose();
    if (Get.isRegistered<IDTextEditingController>()) {
      Get.delete<IDTextEditingController>();
    }
    if (Get.isRegistered<TextEditingController>()) {
      Get.delete<TextEditingController>();
    }
    super.dispose();
  }
}
