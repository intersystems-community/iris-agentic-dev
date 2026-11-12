Class IadSm172Gen.C018 Extends %Persistent [ StorageStrategy = SQLStorage ]
{

Property K1 As %Integer;

Property F1 As %String(COLLATION = "EXACT");

Property F2 As %String;

Index IDKEY On K1 [ IdKey, Unique ];

Storage SQLStorage
{
<SQLMap name="Master">
<Data name="F1">
<Node>"d"</Node>
<Piece>1</Piece>
</Data>
<Data name="F2">
<Node>"d"</Node>
<Piece>2</Piece>
</Data>
<Global>^IadSm172Gen018</Global>
<Subscript name="1">
<Expression>{K1}</Expression>
</Subscript>
<Type>data</Type>
</SQLMap>
<StreamLocation>^IadSm172Gen018S</StreamLocation>
<Type>%Storage.SQL</Type>
}

}
