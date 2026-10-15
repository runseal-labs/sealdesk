import 'package:sealdesk/common.dart';
import 'package:sealdesk/consts.dart';
import 'package:sealdesk/models/platform_model.dart';

class PrinterOptions {
  String action;
  List<String> printerNames;
  String printerName;

  PrinterOptions(
      {required this.action,
      required this.printerNames,
      required this.printerName});

  static PrinterOptions load() {
    var action = bind.mainGetLocalOption(key: kKeyPrinterIncomingJobAction);
    if (![
      kValuePrinterIncomingJobDismiss,
      kValuePrinterIncomingJobDefault,
      kValuePrinterIncomingJobSelected
    ].contains(action)) {
      action = kValuePrinterIncomingJobDefault;
    }

    final printerNames = getPrinterNames();
    var selectedPrinterName = bind.mainGetLocalOption(key: kKeyPrinterSelected);
    if (!printerNames.contains(selectedPrinterName)) {
      if (action == kValuePrinterIncomingJobSelected) {
        action = kValuePrinterIncomingJobDefault;
        bind.mainSetLocalOption(
            key: kKeyPrinterIncomingJobAction,
            value: kValuePrinterIncomingJobDefault);
        if (printerNames.isEmpty) {
          selectedPrinterName = '';
        } else {
          selectedPrinterName = printerNames.first;
        }
        bind.mainSetLocalOption(
            key: kKeyPrinterSelected, value: selectedPrinterName);
      }
    }

    return PrinterOptions(
        action: action,
        printerNames: printerNames,
        printerName: selectedPrinterName);
  }
}
