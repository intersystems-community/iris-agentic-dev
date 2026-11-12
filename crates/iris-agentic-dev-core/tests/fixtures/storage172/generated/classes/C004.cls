Class IadSm172Gen.C004 Extends %Persistent [ StorageStrategy = SQLStorage ]
{

Property K1 As %Integer;

Property F1 As %String(COLLATION = "EXACT");

Property F2 As %String;

Property F3 As %Integer;

Index IDKEY On K1 [ IdKey, Unique ];

Storage SQLStorage
{
<SQLMap name="Master">
<Data name="F1">
<Node>"a"</Node>
<Piece>1</Piece>
</Data>
<Data name="F2">
<Node>"b"</Node>
<Piece>1</Piece>
</Data>
<Data name="F3">
<RetrievalCode> s {*}={L1}</RetrievalCode>
</Data>
<Global>^IadSm172Gen004</Global>
<Subscript name="1">
<AccessType>Sub</AccessType>
<Expression>{K1}</Expression>
</Subscript>
<Type>data</Type>
</SQLMap>
<StreamLocation>^IadSm172Gen004S</StreamLocation>
<Type>%Storage.SQL</Type>
}

}
