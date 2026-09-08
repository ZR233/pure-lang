String modelProtocolLabel(String protocol) => switch (protocol) {
  'responses' => 'Responses',
  'chat_completions' => 'Chat Completions',
  _ => protocol,
};

String modelConnectionLabel(String mode) => switch (mode) {
  'web_socket' => 'WS',
  'http' => 'HTTP',
  _ => mode,
};
